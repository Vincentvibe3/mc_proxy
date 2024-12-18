use std::{io::ErrorKind, sync::Arc};

use bytes::{Buf, Bytes, BytesMut};
use mc_proxy_lib::packet::{self, create_packet};
use tokio::{io::Interest, net::{tcp::{OwnedReadHalf, OwnedWriteHalf}, TcpStream}, sync::{mpsc, RwLock}};
use uuid::Uuid;

use crate::state::State;

pub async fn handshake_tunnel_socket(socket: TcpStream, state: Arc<RwLock<State>>) {
    // TODO add timeout
    let mut is_tunnel = false;
    println!("received tunnel connection");
	// write handshake
	let mut packet = create_packet(&[0;0], 0);
	loop {
		socket.writable().await;
		match socket.try_write(&packet) {
			Ok(0) => return,
			Ok(n) => {
				packet.advance(n);
			}
			Err(_e) => {
				return;
			}
		}
		if packet.is_empty() {
			break;
		}
	}

   // receive handshake
   let mut buffer = BytesMut::with_capacity(4096);
   let mut destination:Option<Uuid> = None;
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
		   is_tunnel = packet.id != 0;
            if is_tunnel {
                destination = Some(Uuid::from_slice(packet.payload).unwrap());
            }
		   break;
	   }
   }

    if is_tunnel {
        let destination = destination.unwrap();
        let mut state = state.write().await;
        state.link_pending_connection(destination, socket);
    } else {
        // write hostname
        let hostname = "mcsrv.local".to_string();
        let hostname_bytes = hostname.as_bytes();
        let mut packet = create_packet(hostname_bytes, 1);
        loop {
            socket.writable().await;
            match socket.try_write(&packet) {
                Ok(0) => return,
                Ok(n) => {
                    packet.advance(n);
                }
                Err(_e) => {
                    return;
                }
            }
            if packet.is_empty() {
                break;
            }
        }

        let (reader, writer) = socket.into_split();
        let mut state_unlocked = state.write().await;
        let receiver = state_unlocked.create_tunnel(&hostname);
        drop(state_unlocked);
        let state_2 = state.clone();
        let host2 = hostname.clone();
        tokio::spawn(async move {
            tunnel_client_keep_alive(reader, state, hostname).await;
        });
        tokio::spawn(async move {
            reverse_connection_request_listener(writer, receiver, state_2, host2).await;
        });
        println!("tunnel ready");
    }
}

// TODO handle client keep-alive signal
async fn tunnel_client_keep_alive(
    reader: OwnedReadHalf,
    state: Arc<RwLock<State>>,
    host: String,
) {
    let mut buf = BytesMut::with_capacity(4096);
    loop {
        reader.readable().await;
        match reader.try_read_buf(&mut buf){
            Ok(0) => {
                println!("tunnnel dead");
                break
            },
            Ok(n) =>  {
                println!("tunnel read {}", n);
            },
            Err(e) => break
        }
        let remove = false;
        if remove {
            let mut unlocked_state = state.write().await;
            unlocked_state.remove_tunnel(host.clone());
        }
    }
}

pub async fn reverse_connection_request_listener(
    writer: OwnedWriteHalf,
    mut request_receiver: mpsc::Receiver<Uuid>,
    state: Arc<RwLock<State>>,
    host: String,
) {
    let mut packet: Option<BytesMut> = None;
    loop {
        if packet == None {
            match request_receiver.recv().await {
                Some(uuid) => {
                    println!("uuid to send {}", uuid);
                    let packet_bytes = create_packet(uuid.as_bytes(), 2);                    
                    packet = Some(packet_bytes);
                    println!("got request");
                }
                None => {
                    println!("channel closed");
                    break;
                }
            }
        }
        if let Some(ref mut packet_bytes) = packet {
            writer.writable().await.unwrap();
            
            match writer.try_write(&packet_bytes) {
                Ok(0) => {
                    println!("eof");
                    break;
                },
                Ok(n) => {
                    packet_bytes.advance(n);
                }
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                    continue;
                }
                Err(_) => {
                    break;
                }
            }
            if packet_bytes.is_empty() {
                println!("wrote packet");
                packet = None
            }
        }
    }
    println!("removing tunnel");
    let mut unlocked_state = state.write().await;
    unlocked_state.remove_tunnel(host);
}
