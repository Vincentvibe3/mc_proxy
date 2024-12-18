use std::{io::ErrorKind, net::ToSocketAddrs};

use bytes::{Buf, BytesMut};
use mc_proxy_lib::packet::{create_packet, get_packet};
use tokio::{io::{copy_bidirectional, AsyncReadExt, AsyncWriteExt}, net::{TcpSocket, TcpStream}};
use uuid::Uuid;


const TUNNEL_PORT:&str = "25567";
const MC_PORT: &str = "25566";
const PROXY_LOCATION: &str = "127.0.0.1";//"proxy.mcproxy.vincentvibe3.com";

async fn start_forwarding_connection(connection_id:Uuid){
	let mut forwarding = TcpStream::connect(PROXY_LOCATION.to_owned()+":"+TUNNEL_PORT).await.unwrap();
	let mut buffer = BytesMut::with_capacity(4096);
	loop {
		let buffer_capacity = buffer.capacity();
		if buffer.len() == buffer_capacity{
			buffer.reserve(buffer_capacity);
		}
		forwarding.readable().await;
		match forwarding.try_read_buf(&mut buffer) {
			Ok(0) => {
				println!("dead");
				break
			},
			Ok(n) => {
				println!("read {}", n)
			}
			Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
				continue;
			}
			Err(e) => {
				eprintln!("error {}", e);
				break
			}
		}
		if let Some(packet) = get_packet(&buffer) {
			match packet.id {
				0 => {
					buffer.advance(packet.size);
					break;
				},
				_ => {}
			}
			buffer.advance(packet.size);
		}
	}
	let mut server = TcpStream::connect("127.0.0.1".to_owned()+":"+MC_PORT).await.unwrap();
	println!("forwarding starting");
	let sent_packet = create_packet(connection_id.as_bytes(), 1);
	forwarding.write_all(&sent_packet).await.unwrap();
	println!("notified new connection");
	// wait for ready
	loop {
		let buffer_capacity = buffer.capacity();
		if buffer.len() == buffer_capacity{
			buffer.reserve(buffer_capacity);
		}
		forwarding.readable().await;
		match forwarding.try_read_buf(&mut buffer) {
			Ok(0) => {
				println!("dead");
				break
			},
			Ok(n) => {
				println!("read 2 {}", n);
				println!("buf {:?}", &buffer);
			}
			Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
				continue;
			}
			Err(e) => {
				eprintln!("error {}", e);
				break
			}
		}
		if let Some(packet) = get_packet(&buffer) {
			match packet.id {
				3 => {
					println!("packet found");
					buffer.advance(packet.size);
					break;
				},
				_ => {
					println!("unknown");
				}
			}
			buffer.advance(packet.size);
		}
	}
	println!("start proxying");
	server.writable().await;
	server.write_all(&buffer).await;
	match copy_bidirectional(&mut forwarding, &mut server).await {
        Ok((n1, n2)) => {
			println!("{} {}: bidirectional", n2, n2);
		}
        Err(e) => eprintln!("An error occured proxying tunnel: {}", e),
    }
}

#[tokio::main]
async fn main() {
	let mut stream = TcpStream::connect(PROXY_LOCATION.to_owned()+":"+TUNNEL_PORT).await.unwrap();
	println!("starting");
	let mut buffer = BytesMut::with_capacity(4096);
	let mut assigned_hostname = "".to_string();
    loop {
		println!("buf len {}", buffer.len());
		let buffer_capacity = buffer.capacity();
		if buffer.len() == buffer_capacity{
			buffer.reserve(buffer_capacity);
		}
		stream.readable().await;
		match stream.try_read_buf(&mut buffer) {
			Ok(0) => {
				println!("dead");
				break
			},
			Ok(n) => {}
			Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
				continue;
			}
			Err(e) => {
				eprintln!("error {}", e);
				break
			}
		}
		while let Some(packet) = get_packet(&buffer) {
			match packet.id {
				0 => {
					let sent_packet = create_packet(&[0;0], 0);
					stream.write_all(&sent_packet).await.unwrap();
					// println!("sent handshake");
				},
				1 => {
					let hostname = String::from_utf8(packet.payload.to_vec()).unwrap();
					assigned_hostname = hostname.clone();
					println!("assigned host {}", hostname);
				},
				2 => {
					let connection_id = Uuid::from_slice(&packet.payload).unwrap();
					println!("creating new connection");
					tokio::spawn(async move {
						start_forwarding_connection(connection_id).await;
					});
				},
				_ => {}
			}
			buffer.advance(packet.size);
		}
	}
}