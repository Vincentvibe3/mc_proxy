use std::{collections::HashMap, str::FromStr, sync::Arc, time::Duration};

use bytes::{Buf, BytesMut};
use mc_proxy_lib::packet::{create_packet, get_packet, read_string, read_varint};
use tokio::{io::{AsyncReadExt, AsyncWriteExt, Interest}, net::{tcp::{OwnedReadHalf, OwnedWriteHalf}, TcpStream}, sync::{Mutex, RwLock}};
use uuid::Uuid;

const TUNNEL_PORT:&str = "25567";
const CLIENT_PORT: &str = "25565";

async fn handle_client_connection(
	mut from_client: OwnedReadHalf, mut to_proxy:OwnedWriteHalf, mut data:BytesMut
){
	println!("client read started");
	loop {
		// tokio::time::sleep(Duration::from_millis(1)).await;
		println!("loop 2");
		let read_ready = from_client.ready(Interest::READABLE);
		let write_ready = to_proxy.ready(Interest::WRITABLE);
		tokio::select! {
			ready = read_ready => {
				if ready.as_ref().unwrap().is_read_closed() {
					break;
				}
				if ready.unwrap().is_readable() {
					match from_client.try_read_buf(&mut data) {
						Ok(n) => {
							if n == 0{
								data.reserve(data.len()+1024);
								continue;
							} else {
								println!("c -> p reading {n}");
							}
						},
						Err(_)=>{}
					}
				}
			}
			ready = write_ready => {
				if ready.as_ref().unwrap().is_write_closed() {
					break;
				}
				if ready.unwrap().is_writable(){
					match to_proxy.try_write(&data) {
						Ok(bytes_written) => {
							if (bytes_written > 0){
								println!("c-> p writing {bytes_written}");
							}
							data.advance(bytes_written);
						},
						Err(_) => {}
					}
				}
			}
		}
	}
	println!("close c->p");
	to_proxy.shutdown().await.unwrap();
}

async fn setup_client_connection(
	stream:TcpStream, 
	clients:Arc<RwLock<HashMap<String, Arc<Mutex<OwnedWriteHalf>>>>>, 
	forwarded_connections:Arc<RwLock<HashMap<Uuid, Arc<OwnedWriteHalf>>>>,
	tunnels:Arc<RwLock<HashMap<Uuid, Arc<Mutex<OwnedWriteHalf>>>>>
){
	let (mut read, write) = stream.into_split();
	let shared_write:Arc<OwnedWriteHalf> = Arc::new(write);
	let uuid = Uuid::new_v4();
	let mut locked_forwarded_connections = forwarded_connections.write().await;
	let mut hostname:String = "".to_string();
	locked_forwarded_connections.insert(uuid, shared_write);
	drop(locked_forwarded_connections);
	let mut data = BytesMut::new();
	loop {
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
		
		let mut locked_tunnels = tunnels.write().await;
		match locked_tunnels.remove(&uuid) {
			Some(received_tunnel) => {
				println!("getting tunnel");
				let mutex = Arc::try_unwrap(received_tunnel).unwrap();
				tokio::spawn(async move {
					handle_client_connection(read, mutex.into_inner(), data).await;
				});
				return;
			},
			None => {}
		}
	}
}

async fn handle_tunnel_connection(
	mut read: OwnedReadHalf, mut data:BytesMut, mut forwarding_writer:OwnedWriteHalf
){
	println!("forwarding started");
	loop {
		// tokio::time::sleep(Duration::from_millis(1)).await;
		println!("loop");
		let read_ready = read.ready(Interest::READABLE);
		let write_ready = forwarding_writer.ready(Interest::WRITABLE);
		tokio::select! {
			ready = read_ready => {
				if ready.as_ref().unwrap().is_read_closed() {
					break;
				}
				if ready.unwrap().is_readable() {
					match read.try_read_buf(&mut data) {
						Ok(n) => {
							if n == 0{
								data.reserve(data.len()+1024);
								continue;
							} else {
								println!("p->c reading {n}");
							}
						},
						Err(_)=>{}
					}
				}
			}
			ready = write_ready => {
				if ready.as_ref().unwrap().is_write_closed() {
					break;
				}
				if ready.unwrap().is_writable(){
					match forwarding_writer.try_write(&data) {
						Ok(bytes_written) => {
							if (bytes_written > 0){
								println!("p->c writing {bytes_written}");
							}
							data.advance(bytes_written);
						},
						Err(_) => {}
					}
				}
			}
		}
	}
	println!("close p->c");
	forwarding_writer.shutdown().await.unwrap();
}

async fn setup_tunnel_connection(
	stream:TcpStream, 
	clients:Arc<RwLock<HashMap<String, Arc<Mutex<OwnedWriteHalf>>>>>, 
	forwarded_connections:Arc<RwLock<HashMap<Uuid, Arc<OwnedWriteHalf>>>>,
	tunnels:Arc<RwLock<HashMap<Uuid, Arc<Mutex<OwnedWriteHalf>>>>>
){
	let (mut read, write) = stream.into_split();
	write.writable().await.unwrap();
	let mut bytes_written = 0;
	let packet = create_packet(&[0;0], 0);
	while bytes_written < packet.len(){
		let n = write.try_write(&packet).unwrap();
		bytes_written += n;
	}
	let mut typefound = false;
	println!("wrote handshake");
	let writer = Arc::new(Mutex::new(write));
	let mut data = BytesMut::new();
	loop {
		read.readable().await.unwrap();
		match read.read_buf(&mut data).await {
			Ok(n) => {
				if n == 0{
					data.reserve(data.len()+1024);
					continue;
				}
				let bytes = data.clone().freeze();
				if !typefound {
					match get_packet(&bytes){
						Ok(packet) => {
							println!("got client packet");
							if packet.id == 0 {
								println!("Broadcast channel set");
								let external_hostname = String::from_str("mcsrv.local").unwrap();
								let external_hostname_bytes = external_hostname.as_bytes();
								let mut clients_locked = clients.write().await;
								clients_locked.insert(external_hostname.clone(), writer.clone());
								let packet_to_write = create_packet(external_hostname_bytes, 1);
								let mut bytes_written = 0;
								let locked_writer = writer.lock().await;
								while bytes_written < packet_to_write.len(){
									let n = locked_writer.try_write(&packet_to_write).unwrap();
									bytes_written += n;
								}
								data.advance(packet.size);
								return;
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
									handle_tunnel_connection(read, data, forwarding_writer).await
								});
								return;
							}
						},
						Err(_) => {
							continue;
						}
					}
				}
			},
			Err(_) => {}
		}
	}
}

async fn start_client_listener(
	connected_clients: Arc<RwLock<HashMap<String, Arc<Mutex<OwnedWriteHalf>>>>>,
	forwarded_connections: Arc<RwLock<HashMap<Uuid, Arc<OwnedWriteHalf>>>>,
	tunnels: Arc<RwLock<HashMap<Uuid, Arc<Mutex<OwnedWriteHalf>>>>>,
){
	let listener = tokio::net::TcpListener::bind("0.0.0.0:".to_owned()+CLIENT_PORT).await.unwrap();
	loop {
        let socket = listener.accept().await.unwrap();
		println!("getting new client connection");
		let clients = connected_clients.clone();
		let forwarded_connections = forwarded_connections.clone();
		let tunnels = tunnels.clone();
        tokio::spawn(async move {
            let stream = socket.0;
            setup_client_connection(stream, clients, forwarded_connections, tunnels).await;
        });

    }
}

async fn start_tunnel_listener(
	connected_clients: Arc<RwLock<HashMap<String, Arc<Mutex<OwnedWriteHalf>>>>>,
	forwarded_connections: Arc<RwLock<HashMap<Uuid, Arc<OwnedWriteHalf>>>>,
	tunnels: Arc<RwLock<HashMap<Uuid, Arc<Mutex<OwnedWriteHalf>>>>>,
){
	// start forwarding tcp listener
	let listener = tokio::net::TcpListener::bind("0.0.0.0:".to_owned()+TUNNEL_PORT).await.unwrap();
    loop {
        let socket = listener.accept().await.unwrap();
		println!("getting new connection");
		let clients = connected_clients.clone();
		let forwarded_connections = forwarded_connections.clone();
		let shared_tunnels = tunnels.clone();
        tokio::spawn(async move {
            let stream = socket.0;
            setup_tunnel_connection(stream, clients, forwarded_connections, shared_tunnels).await;
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
	start_tunnel_listener(connected_clients, forwarded_connections, tunnels).await;
}