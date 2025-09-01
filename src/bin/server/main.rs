use std::{collections::{HashMap, VecDeque}, error::Error, fs::File, io::{self, IoSlice, Read, Write}, net::{IpAddr, Ipv4Addr, SocketAddr}, ops::SubAssign, sync::Arc, time::Duration};

use bytes::{Buf, BufMut, Bytes, BytesMut};
use futures::{future::poll_fn, select, SinkExt};
use mc_proxy_lib::{packet::{self, create_packet, get_packet, read_string, read_varint}, tunnel::{quic_to_tcp, tcp_to_quic}};
use quinn::{Chunk, Connection, Endpoint, RecvStream, SendStream, ServerConfig};
use rcgen::CertifiedKey;
use rustls::{pki_types::{CertificateDer, PrivatePkcs8KeyDer}, server};
use tokio::{io::AsyncWriteExt, net::{tcp::{OwnedReadHalf, OwnedWriteHalf}, TcpStream}, sync::{mpsc::{self, Receiver}, Mutex, RwLock}, time::sleep};


const SERVER_NAME: &str = "test.mcproxy.vincentvibe3.com";
const LOCALHOST_V4: IpAddr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
const CLIENT_ADDR: SocketAddr = SocketAddr::new(LOCALHOST_V4, 5000);
const SERVER_ADDR: SocketAddr = SocketAddr::new(LOCALHOST_V4, 5001);
const CLIENT_PORT:&str = "25565";

fn save_cert_to_file(cert:&String, private_key:&String) -> std::io::Result<()>{
    let mut file_cert = File::create("server.crt")?;
    file_cert.write_all(cert.as_bytes())?;
    let mut file_pk = File::create("server.pem")?;
    file_pk.write_all(private_key.as_bytes())?;
    Ok(())
}

fn generate_self_signed_cert()
-> Result<(CertificateDer<'static>, PrivatePkcs8KeyDer<'static>), Box<dyn Error>> {
    let cert = rcgen::generate_simple_self_signed(vec![SERVER_NAME.to_string()])?;
    println!("{}", cert.cert.pem());
    println!("{}", cert.key_pair.serialize_pem());
    save_cert_to_file(&(cert.cert.pem()), &(cert.key_pair.serialize_pem()))?;
    let cert_der = CertificateDer::from(cert.cert);
    let key = PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der());
    Ok((cert_der, key))
}

async fn handle_tunnel_client(conn:Connection, connections:Arc<RwLock<HashMap<String, Connection>>>) -> Result<(), Box<dyn Error>> {
    println!("new client detected");
    // create event stream
    let stream = conn.accept_bi().await?;
    let mut send = stream.0;
    let mut recv = stream.1;
    // send assigned subdomain
    let mut buffer = BytesMut::with_capacity(4096);
    let mut setup = false;
    loop {
        let chunk = recv.read_chunk(4096, true).await?.unwrap();
        let current_buffer_capacity = buffer.capacity();
        if buffer.len() > current_buffer_capacity - chunk.bytes.len() {
            buffer.reserve(current_buffer_capacity);
        }
        buffer.put(chunk.bytes);

        if let Some(packet) = packet::get_packet(&buffer) {
            println!("found packet");
            if packet.id == 0 {
                // handshake
                let subdomain = "test.mcproxy.vincentvibe3.com";
                let subdomain_bytes = subdomain.as_bytes();
                let handshake_packet = create_packet(subdomain_bytes, 0);
                send.write_chunk(handshake_packet.freeze()).await?;
                let mut connections_list = connections.write().await;
                connections_list.insert(subdomain.to_string(), conn);
                setup = true;
            }
            else if packet.id == 1 {
                // unusused
            } else if packet.id == 2 {
                //keep-alive
            }
            buffer.advance(packet.size);
		    break;
	   }
       if setup {
            sleep(Duration::new(15, 0)).await;
       }
    }
    Ok(())
}

async fn handle_connection(mut stream:TcpStream, connections:Arc<RwLock<HashMap<String, Connection>>>)-> Result<(), Box<dyn Error>>{
    let mut hostname = "".to_string();
    let mut data = BytesMut::with_capacity(4096);
    while hostname == "" {
        stream.readable().await?;
        if data.len() == data.capacity(){
            data.reserve(data.len()+1024);
        }
        match stream.try_read_buf(&mut data) {
            Ok(0) => break,
            Ok(n) => {
                // println!("read {} bytes", n);
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                continue;
            }
            Err(e) => {
                return Err(e.into());
            }
        }
        if let Some(packet) = packet::get_packet(&data) {
            let (_, bytes_read) = read_varint(&packet.payload);
            let (read_hostname, _) = read_string(&packet.payload[bytes_read..]);
            hostname = read_hostname;
            println!("set hostname {hostname}");
        }
    }
    let connection_list = connections.read().await;
    if let Some(connection) = connection_list.get(&hostname){
        let split = stream.into_split();
        let read = split.0;
        let write = split.1;
        let quic_stream = connection.open_bi().await.unwrap();
        let send = quic_stream.0;
        let recv = quic_stream.1;
        tokio::spawn(async move {
            quic_to_tcp(recv, write).await;
        });
        tcp_to_quic(read, send, data).await?;
    } else {
        stream.shutdown().await?;
    }
    Ok(())
}



async fn setup_tcp_server(connections:Arc<RwLock<HashMap<String, Connection>>>){
    let listener = tokio::net::TcpListener::bind("0.0.0.0:25565").await.unwrap();
    loop {
        let socket = listener.accept().await.unwrap();
		println!("getting new connection");
		let clients = connections.clone();
        tokio::spawn(async move {
            let stream = socket.0;
            handle_connection(stream, clients).await.unwrap();
        });

    }
}

async fn setup_quic_server(connections:Arc<RwLock<HashMap<String, Connection>>>) -> Result<(), Box<dyn Error>>{
    let certs = generate_self_signed_cert().unwrap();
	let mut server_config = quinn::ServerConfig::with_single_cert(vec![certs.0.clone()], certs.1.into()).unwrap();
    let transport_config = Arc::get_mut(&mut server_config.transport).unwrap();
    transport_config.max_concurrent_bidi_streams(255_u8.into());
	let endpoint = Endpoint::server(server_config, SERVER_ADDR)?;

    // Start iterating over incoming connections.
    while let Some(conn) = endpoint.accept().await {
        let connection = conn.await?;
        let connections_list = connections.clone();
        tokio::spawn(async move {
            handle_tunnel_client(connection, connections_list).await.unwrap();
        });
        // Save connection somewhere, start transferring, receiving data, see DataTransfer tutorial.
    }
    Ok(())
}

#[tokio::main()]
async fn main() -> Result<(), Box<dyn Error>> {
    let connections_map:HashMap<String, Connection> = HashMap::new();
    let connections = Arc::new(RwLock::new(connections_map));
    let connections2 = connections.clone();
	tokio::spawn(async move {
        setup_quic_server(connections).await;
    });
    setup_tcp_server(connections2).await;
    Ok(())
}