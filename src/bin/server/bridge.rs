use std::io::ErrorKind;

use bytes::{Buf, BytesMut};
use mc_proxy_lib::packet::create_packet;
use tokio::{io::{copy_bidirectional, AsyncWriteExt}, net::{tcp::{OwnedReadHalf, OwnedWriteHalf}, TcpStream}};

pub async fn tunnel_streams(client: TcpStream, tunnel: TcpStream) {
    let (client_read, client_write) = client.into_split();
    let (tunnel_read, tunnel_write) = tunnel.into_split();
    tokio::spawn(async move {
        copy_to_stream(client_read,tunnel_write).await;
    });
    copy_to_stream(tunnel_read, client_write).await;
    // match copy_bidirectional(&mut client, &mut tunnel).await {
    //     Ok((n1, n2)) => {
	// 		println!("{} {}: bidirectional", n2, n2);
	// 	},
    //     Err(e) => eprintln!("An error occured proxying tunnel: {}", e),
    // }
}

pub async fn copy_to_stream(sending_stream: OwnedReadHalf, receiving_stream: OwnedWriteHalf){
    let buffer_capacity = 8192;
    let mut buffer =  BytesMut::with_capacity(8192);
    loop {
        if buffer.len() < buffer_capacity{
            sending_stream.readable().await;
            match sending_stream.try_read_buf(&mut buffer){
                Ok(0) => break,
                Ok(n) => {},
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                    continue;
                }
                Err(e) => break,
            }
        }
        receiving_stream.writable().await;
        match receiving_stream.try_write(&buffer) {
            Ok(0) => break,
            Ok(n) => {
                buffer.advance(n);
            },
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                continue;
            }
            Err(e) => break,
        }
    }
}
