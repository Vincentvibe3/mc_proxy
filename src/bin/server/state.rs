use std::collections::HashMap;

use tokio::{net::TcpStream, sync::{mpsc::{self}, oneshot::{self}}};
use uuid::Uuid;

pub struct State {
	tunnel_clients:HashMap<String, mpsc::Sender<Uuid>>,
	connections:HashMap<Uuid, oneshot::Sender<TcpStream>>
}

impl State {

	pub fn new() -> State{
		return State {
			tunnel_clients:HashMap::new(),
			connections:HashMap::new()
		}
	}

	pub fn create_tunnel(&mut self, host:&String) -> mpsc::Receiver<Uuid>{
		let (sender, receiver) = mpsc::channel::<Uuid>(16);
		self.tunnel_clients.insert(host.to_string(), sender);
		println!("added {}", host);
		println!("new clients list, {:?}", self.tunnel_clients.clone().into_keys());
		return receiver;
	}

	pub fn remove_tunnel(&mut self, host:String){
		println!("tunnel {} removed", host);
		self.tunnel_clients.remove(&host);
	}

	pub fn link_pending_connection(&mut self, id:Uuid, socket:TcpStream){
		println!("uuid {}", id);
		let channel = self.connections.remove(&id).unwrap();
		channel.send(socket);
	}

	pub async fn add_pending_connection(&mut self, host:String) -> oneshot::Receiver<TcpStream> {
		let id = Uuid::new_v4();
		println!("created uuid {}", id);
		println!("getting {}", host);
		println!("{:?}", self.tunnel_clients.clone().into_keys());
		let new_connection_channel = self.tunnel_clients.get(&host).unwrap();
		new_connection_channel.send(id).await;
		let (send, receive) = oneshot::channel();
		self.connections.insert(id, send);
		return receive;
	}

}