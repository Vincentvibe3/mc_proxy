use std::{collections::HashMap, fs::read, io::Read, sync::Arc, time::Duration};

use bytes::{Buf, BytesMut};
use mc_proxy_lib::packet::{create_forwarded_packet, create_packet, create_varint, get_packet};
use uuid::Uuid;
use tokio::{io::{AsyncReadExt, AsyncWriteExt, Interest}, net::{tcp::{OwnedReadHalf, OwnedWriteHalf}, TcpSocket}, sync::Mutex};

const TUNNEL_PORT:&str = "25567";
const MC_PORT: &str = "25566";

async fn create_new_connection(addr: String) -> (OwnedReadHalf, OwnedWriteHalf){
	let addr = addr.parse().unwrap();
    let socket = TcpSocket::new_v4().unwrap();
    let stream = socket.connect(addr).await.unwrap();
	let (read, write) = stream.into_split();
	return (read, write);
}

async fn tunnel_handler(mut from_proxy:OwnedReadHalf, mut to_server:OwnedWriteHalf, mut data:BytesMut){
	loop {
		let read_ready = from_proxy.ready(Interest::READABLE);
		let write_ready = to_server.ready(Interest::WRITABLE);
		tokio::select! {
			ready = read_ready => {
				if ready.as_ref().unwrap().is_read_closed(){
					break;
				}
				if ready.unwrap().is_readable(){
					match from_proxy.try_read_buf(&mut data) {
						Ok(n) => {
							if n == 0{
								data.reserve(data.len()+1024);
								continue;
							} else {
								println!("p -> s read {n}");
							}
						},
						Err(_) => {}
					}
				}
			}
			ready = write_ready => {
				if ready.as_ref().unwrap().is_write_closed() {
					break;
				}
				if ready.unwrap().is_writable() {
					match to_server.try_write(&data) {
						Ok(bytes_written) => {
							if bytes_written > 0 {
								println!("p -> s write {bytes_written}");
							}
							data.advance(bytes_written);
						},
						Err(_) => {}
					}
				}
			}
		}
	}
	println!("close p->s");
	to_server.shutdown().await.unwrap();
}

async fn forwarding_handler(mut from_server: OwnedReadHalf, mut to_proxy: OwnedWriteHalf){
	let mut data = BytesMut::new();
	loop {
		let read_ready = from_server.ready(Interest::READABLE);
		let write_ready = to_proxy.ready(Interest::WRITABLE);
		tokio::select! {
			ready = read_ready => {
				if ready.as_ref().unwrap().is_read_closed(){
					break;
				}
				if ready.unwrap().is_readable() {
					match from_server.try_read_buf(&mut data) {
						Ok(n) => {
							if n == 0{
								data.reserve(data.len()+1024);
								continue;
							} else {
								println!("s -> p read {n}");
							}
						},
						Err(_) => {}
					}
				}

			},
			ready = write_ready => {
				if ready.as_ref().unwrap().is_write_closed() {
					break;
				}
				if ready.unwrap().is_writable() {
					match to_proxy.try_write(&data) {
						Ok(bytes_written) => {
							if bytes_written > 0 {
								println!("s -> p write {bytes_written}");
							}
							data.advance(bytes_written);
						},
						Err(_) => {}
					}
				}
			}
		}
	}
	println!("close s->p");
	to_proxy.shutdown().await.unwrap();
}

async fn create_tunnel(uuid:Uuid){
	let (read, write) = create_new_connection("127.0.0.1:".to_owned()+MC_PORT).await;
	let (mut tunnel_read, mut tunnel_write) = create_new_connection("proxy.mcproxy.vincentvibe3.com:".to_owned()+TUNNEL_PORT).await;
	let mut data = BytesMut::new();
	loop {
		tunnel_read.readable().await.unwrap();
		match tunnel_read.read_buf(&mut data).await {
			Ok(n) => {
				if n == 0{
					data.reserve(data.len()+1024);
					continue;
				}
				let bytes = data.clone().freeze();

				match get_packet(&bytes){
					Ok(packet) => {
						if packet.id == 0 {
							println!("forwarding handshake received");
							let handshake_packet = create_packet(uuid.as_bytes(), 1);
							tunnel_write.write_all(&handshake_packet).await.unwrap();
							println!("response sent");
							data.advance(packet.size);
							break;
						}
					},
					Err(_) => {}
				}
			},
			Err(_) => {}
		}
	}
	tokio::spawn(async move {
		forwarding_handler(read, tunnel_write).await;
	});
	tokio::spawn(async move {
		tunnel_handler( tunnel_read, write, data).await;
	});
}

#[tokio::main]
async fn main() {
	let addr = "127.0.0.1:25567".parse().unwrap();
    let socket = TcpSocket::new_v4().unwrap();
    let stream = socket.connect(addr).await.unwrap();
	let (mut read, mut write) = stream.into_split();
	// write.writable().await.unwrap();
	// let packet = create_packet(&[], 0);
	// write.write_all(&packet).await.unwrap();
	let mut shared_write = write;
	let mut users: HashMap<Uuid, OwnedWriteHalf> = HashMap::new();
	let mut handshake_done = false;

	let mut data = BytesMut::new();
	loop {
		println!("punch through read loop");
		read.readable().await.unwrap();
		match read.read_buf(&mut data).await {
			Ok(n) => {
				if n == 0{
					data.reserve(data.len()+1024);
					continue;
				}
				let bytes = data.clone().freeze();
				match get_packet(&bytes){
					Ok(packet) => {
						if packet.id == 0 {
							let handshake_packet = create_packet(&[0;0], 0);
							shared_write.write_all(&handshake_packet).await.unwrap();
							handshake_done = true;
							println!("handshake done");
						}else if packet.id == 1 {
							let hostname = String::from_utf8(packet.payload.to_vec()).unwrap();
							println!("{hostname}");
						} else if packet.id == 2 {
							let uuid = Uuid::from_slice(&packet.payload[..16]).unwrap();
							tokio::spawn(async move {
								create_tunnel(uuid).await;
							});
						}
						data.advance(packet.size);
					},
					Err(_) => {
						continue;
					}
				}
			},
			Err(_) => {

			}
		}
	}
    
}