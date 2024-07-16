use std::{borrow::Borrow, cmp::min, collections::HashMap, io::Read, str::FromStr, sync::Arc, time::Duration};

use futures::lock;
use mc_proxy_lib::packet::{create_forwarded_packet, create_packet, get_packet, read_string, read_varint};
use tokio::{io::{AsyncReadExt, AsyncWriteExt, Interest}, net::{tcp::{OwnedReadHalf, OwnedWriteHalf}, TcpStream}, sync::{Mutex, RwLock}};
use uuid::Uuid;
use bytes::{Buf, BytesMut};

enum ConnectionType {
	Broadcast,
	Unset,
}

async fn handle_connection(
	stream:TcpStream, 
	clients:Arc<RwLock<HashMap<String, Arc<Mutex<OwnedWriteHalf>>>>>, 
	forwarded_connections:Arc<RwLock<HashMap<Uuid, Arc<OwnedWriteHalf>>>>, 
	tunnels:Arc<RwLock<HashMap<Uuid, Arc<Mutex<OwnedWriteHalf>>>>>
){
	let (mut read, write) = stream.into_split();
	let shared_write:Arc<OwnedWriteHalf> = Arc::new(write);
	let uuid = Uuid::new_v4();
	let mut tunnel:Option<OwnedWriteHalf> = None;
	let mut locked_forwarded_connections = forwarded_connections.write().await;
	let mut hostname:String = "".to_string();
	locked_forwarded_connections.insert(uuid, shared_write);
	drop(locked_forwarded_connections);
	let mut data = BytesMut::new();
	loop {
		println!("client reading");
		tokio::time::sleep(Duration::from_millis(100)).await;
			match read.try_read_buf(&mut data){
				Ok(n) => {
					if n == 0{
						data.reserve(data.len()+1024);
						continue;
					}
					if hostname == "" {
						let bytes = data.clone().freeze();
						match get_packet(&bytes){
							Ok(packet) => {
								let (_, bytes_read) = read_varint(&packet.payload);
								let (read_hostname, _) = read_string(&packet.payload[bytes_read..]);
								hostname = read_hostname;
								println!("set hostname {hostname}");
								let clients_locked = clients.read().await;
								let mut broadcast_stream = clients_locked.get(&hostname).unwrap().lock().await;
								let packet_to_send = create_packet(uuid.as_bytes(), 2);
								broadcast_stream.write_all(&packet_to_send).await.unwrap();
							},
							Err(_) => {
								continue;
							}
						}
					}
				},
				Err(_) => {}
			}
		
		match &mut tunnel {
			Some(forwarding_tunnel) => {
				let data_to_write = data.split();
				forwarding_tunnel.write_all(&data_to_write).await.unwrap();
			},
			None =>{
				let mut locked_tunnels = tunnels.write().await;
				match locked_tunnels.remove(&uuid) {
					Some(received_tunnel) => {
						println!("getting tunnel");
						let mutex = Arc::try_unwrap(received_tunnel).unwrap();
						tokio::spawn(async move {
							connection_loop(read, mutex.into_inner(), data).await;
						});
						return;
					},
					None => {}
				}
			}
		}
	}
}

async fn connection_loop(mut from_client: OwnedReadHalf, mut to_proxy:OwnedWriteHalf, mut data:BytesMut){
	println!("client read started");
	loop {
		let read_ready = from_client.ready(Interest::READABLE);
		let write_ready = to_proxy.ready(Interest::WRITABLE);
		tokio::time::sleep(Duration::from_millis(10)).await;
		tokio::select! {
			biased;
			ready = read_ready => {
				if ready.unwrap().is_readable() {
					match from_client.try_read_buf(&mut data) {
						Ok(n) => {
							println!("c-> p freading");
							if n == 0{
								data.reserve(data.len()+1024);
								continue;
							} 
						},
						Err(_)=>{}
					}
				}
			}
			// ready = write_ready => {
			// 	if ready.unwrap().is_writable(){
			// 		match to_proxy.try_write(&data) {
			// 			Ok(bytes_written) => {
			// 				println!("c-> p fwriting {bytes_written}");
			// 				data.advance(bytes_written);
			// 			},
			// 			Err(_) => {}
			// 		}
			// 	}
			// }
		}
	}
}

async fn handle_client_forwarding_connection(mut read: OwnedReadHalf, mut data:BytesMut, forwarding_writer:OwnedWriteHalf){
	println!("forwarding started");
	loop {
		let read_ready = read.ready(Interest::READABLE);
		let write_ready = forwarding_writer.ready(Interest::WRITABLE);
		tokio::time::sleep(Duration::from_millis(10)).await;
		tokio::select! {
			biased;
			ready = read_ready => {
				if ready.unwrap().is_readable() {
					match read.try_read_buf(&mut data) {
						Ok(n) => {
							println!("p->c reading");
							if n == 0{
								data.reserve(data.len()+1024);
								continue;
							} 
						},
						Err(_)=>{}
					}
				}
			}
			// ready = write_ready => {
			// 	if ready.unwrap().is_writable(){
			// 		match forwarding_writer.try_write(&data) {
			// 			Ok(bytes_written) => {
			// 				println!("p->c writing");
			// 				data.advance(bytes_written);
			// 			},
			// 			Err(_) => {}
			// 		}
			// 	}
			// }
		}
	}
}

async fn handle_client_connection(
	stream:TcpStream, 
	clients:Arc<RwLock<HashMap<String, Arc<Mutex<OwnedWriteHalf>>>>>, 
	forwarded_connections:Arc<RwLock<HashMap<Uuid, Arc<OwnedWriteHalf>>>>,
	tunnels:Arc<RwLock<HashMap<Uuid, Arc<Mutex<OwnedWriteHalf>>>>>
){
	println!("handling client connection");
	let (mut read, write) = stream.into_split();
	write.writable().await.unwrap();
	let mut connection_type = ConnectionType::Unset;
	let mut forwarding_writer:Option<Arc<OwnedWriteHalf>> = None;
	
	let mut bytes_written = 0;
	let packet = create_packet(&[0;0], 0);
	while bytes_written < packet.len(){
		let n = write.try_write(&packet).unwrap();
		bytes_written += n;
	}
	println!("wrote handshake");
	let writer = Arc::new(Mutex::new(write));
	let mut data = BytesMut::new();
	loop {
		read.readable().await.unwrap();
		println!("client read loop");
		match read.read_buf(&mut data).await {
			Ok(n) => {
				if n == 0{
					data.reserve(data.len()+1024);
					continue;
				}
				let bytes = data.clone().freeze();
				match connection_type {
					ConnectionType::Unset => {
						match get_packet(&bytes){
							Ok(packet) => {
								println!("got client packet");
								if packet.id == 0 {
									println!("Broadcast channel set");
									connection_type = ConnectionType::Broadcast;
									let external_hostname = String::from_str("mcsrv.local").unwrap();
									let external_hostname_bytes = external_hostname.as_bytes();
									let mut clients_locked = clients.write().await;
									clients_locked.insert(external_hostname.clone(), writer.clone());
									let packet = create_packet(external_hostname_bytes, 1);
									let mut bytes_written = 0;
									let locked_writer = writer.lock().await;
									while bytes_written < packet.len(){
										let n = locked_writer.try_write(&packet).unwrap();
										bytes_written += n;
									}
								} else if packet.id == 1 {
									println!("set new forwarding");
									let uuid = Uuid::from_slice(&packet.payload[..16]).unwrap();	
									let mut clients_locked = forwarded_connections.write().await;
									// forwarding_writer = Some();							
									let fw = clients_locked.remove(&uuid).unwrap();
									let mut locked_tunnels = tunnels.write().await;
									locked_tunnels.insert(uuid, writer.clone());
									let forwarding_writer = Arc::into_inner(fw).unwrap();
									data.advance(packet.size);
									tokio::spawn(async move {
										println!("starting forward task");
										handle_client_forwarding_connection(read, data, forwarding_writer).await
									});
									return;
								}
								data.advance(packet.size);
							},
							Err(_) => {
								continue;
							}
						}
					},
					ConnectionType::Broadcast => {

					}
				}
			},
			Err(_) => {}
		}
	}
}

async fn start_client_listener(
	clients:Arc<RwLock<HashMap<String, Arc<Mutex<OwnedWriteHalf>>>>>, 
	forwarded_connections:Arc<RwLock<HashMap<Uuid, Arc<OwnedWriteHalf>>>>,
	tunnels:Arc<RwLock<HashMap<Uuid, Arc<Mutex<OwnedWriteHalf>>>>>
){
	let listener = tokio::net::TcpListener::bind("0.0.0.0:25567").await.unwrap();
	loop {
        let socket = listener.accept().await.unwrap();
		println!("getting new client connection");
		let clients = clients.clone();
		let forwarded_connections = forwarded_connections.clone();
		let tunnels = tunnels.clone();
        tokio::spawn(async move {
            let stream = socket.0;
            handle_client_connection(stream, clients, forwarded_connections, tunnels).await;
        });

    }
}

#[tokio::main]
async fn main() {
	let connected_clients: Arc<RwLock<HashMap<String, Arc<Mutex<OwnedWriteHalf>>>>> = Arc::new(RwLock::new(HashMap::new()));
	let tunnels: Arc<RwLock<HashMap<Uuid, Arc<Mutex<OwnedWriteHalf>>>>> = Arc::new(RwLock::new(HashMap::new()));
	let forwarded_connections: Arc<RwLock<HashMap<Uuid, Arc<OwnedWriteHalf>>>> = Arc::new(RwLock::new(HashMap::new()));
	let shared_tunnels = tunnels.clone();
	let shared_connected_clients = connected_clients.clone();
	let shared_forwarded_connections = forwarded_connections.clone();
	//Start client connection task
	tokio::spawn(async move {
		start_client_listener(shared_connected_clients, shared_forwarded_connections, shared_tunnels).await;
	});
	// start forwarding tcp listener
	let listener = tokio::net::TcpListener::bind("0.0.0.0:25568").await.unwrap();
    loop {
        let socket = listener.accept().await.unwrap();
		println!("getting new connection");
		let clients = connected_clients.clone();
		let forwarded_connections = forwarded_connections.clone();
		let shared_tunnels = tunnels.clone();
        tokio::spawn(async move {
            let stream = socket.0;
            handle_connection(stream, clients, forwarded_connections, shared_tunnels).await;
        });

    }
    
}