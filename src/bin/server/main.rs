use state::State;
use tokio::{net::TcpListener, sync::RwLock};

use std::{
    io::{self},
    sync::Arc,
};

mod bridge;
mod client;
mod state;
mod tunnel;

const TUNNEL_PORT: &str = "25567";
const CLIENT_PORT: &str = "25565";

async fn start_tunnel_listener(state: Arc<RwLock<State>>) -> io::Result<()> {
	println!("listening for tunnels");
    let listener = TcpListener::bind("0.0.0.0:".to_string()+TUNNEL_PORT).await?;

    loop {
        let (socket, _) = listener.accept().await?;
        socket.set_nodelay(true)?;
        let pending_connections = state.clone();
        tokio::spawn(async move {
            tunnel::handshake_tunnel_socket(socket, pending_connections).await;
        });
    }
}

async fn start_client_listener(state: Arc<RwLock<State>>) -> io::Result<()> {
	println!("listening for clients");
    let listener = TcpListener::bind("0.0.0.0:".to_string()+CLIENT_PORT).await?;

    loop {
        let (socket, _) = listener.accept().await?;
        socket.set_nodelay(true)?;
        let pending_connections = state.clone();
        tokio::spawn(async move {
            client::handshake_client_socket(socket, pending_connections).await;
        });
    }
}

#[tokio::main()]
async fn main() {
    let connections = Arc::new(RwLock::new(State::new()));
    let pending_connections = connections.clone();
    tokio::spawn(async move {
        if let Err(e) = start_client_listener(pending_connections).await {
			eprintln!("{}", e);
		}
    });
    let pending_connections = connections.clone();
    if let Err(e) = start_tunnel_listener(pending_connections).await {
		eprintln!("{}", e);
	}
}
