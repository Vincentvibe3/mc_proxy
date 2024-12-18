use std::{io::ErrorKind, sync::Arc};

use bytes::{Buf, BytesMut};
use mc_proxy_lib::packet::{self, create_packet, read_string, read_varint};
use tokio::{io::AsyncWriteExt, net::TcpStream, sync::RwLock};

use crate::{bridge::tunnel_streams, state::State};



pub async fn handshake_client_socket(socket: TcpStream, state: Arc<RwLock<State>>) {
    let mut buffer = BytesMut::with_capacity(4096);
    let mut hostname = "".to_string();
    // get initial mc packet
    loop {
        socket.readable().await;
        let current_buffer_capacity = buffer.capacity();
        if buffer.len() == current_buffer_capacity {
            buffer.reserve(current_buffer_capacity);
        }
        match socket.try_read_buf(&mut buffer) {
            Ok(0) => return,
            Ok(n) => {}
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                continue;
            }
            Err(e) => {
                return;
            }
        }
        if let Some(packet) = packet::get_packet(&buffer) {
            let (_protocol_ver, bytes_read) = read_varint(&packet.payload);
            let (read_hostname, _) = read_string(&packet.payload[bytes_read..]);
            hostname = read_hostname;
            break;
        }
    }
    println!("got mc packet");
    println!("hostname: {}", hostname);
    let mut state = state.write().await;
    let receive = state.add_pending_connection(hostname).await;
    drop(state);
    let stream = receive.await;
    println!("received tunnel steam");
    if let Ok(mut stream) = stream {
        stream.writable().await;
        let sent_packet = create_packet(&[0;0], 3);
        println!("sp {:?}", &sent_packet);
        stream.write_all(&sent_packet).await;
        // write already read data to tunnel
        loop {
            stream.writable().await;
            match stream.try_write(&mut buffer) {
                Ok(0) => return,
                Ok(n) => {
                    println!("sent {} bytes of existing data", n);
                    buffer.advance(n);
                }
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                    continue;
                }
                Err(e) => {
                    return;
                }
            }
            if buffer.is_empty() {
                break;
            }
        }
        println!("wrote existing data");
        tunnel_streams(socket, stream).await;
    }
}

