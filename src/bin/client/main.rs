mod certverification;

use std::{error::Error, fs::File, io::Read, net::{IpAddr, Ipv4Addr, SocketAddr}, sync::Arc};

use bytes::{Buf, BufMut, Bytes, BytesMut};
use mc_proxy_lib::{packet::{create_packet, get_packet}, tunnel::{quic_to_tcp, tcp_to_quic}};
use quinn::{crypto::rustls::{NoInitialCipherSuite, QuicClientConfig}, ClientConfig, Connection, Endpoint};
use rustls::pki_types::{pem::PemObject, CertificateDer};
use tokio::net::TcpStream;

use crate::certverification::SkipServerVerification;

const SERVER_NAME: &str = "localhost";
const LOCALHOST_V4: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const CLIENT_ADDR: SocketAddr = SocketAddr::new(LOCALHOST_V4, 5000);
const SERVER_ADDR: SocketAddr = SocketAddr::new(LOCALHOST_V4, 5001);
const TUNNEL_PORT:&str = "25567";
const MC_PORT: &str = "25566";
const PROXY_LOCATION: &str = "127.0.0.1";//"proxy.mcproxy.vincentvibe3.com";//

fn configure_insecure_client() -> Result<ClientConfig, NoInitialCipherSuite> {
    let crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(SkipServerVerification::new())
        .with_no_client_auth();

    Ok(ClientConfig::new(Arc::new(QuicClientConfig::try_from(
        crypto,
    )?)))
}

fn configure_client() -> Result<ClientConfig, Box<dyn Error + Send + Sync + 'static>>{
    let mut certs = rustls::RootCertStore::empty();
    let cert = CertificateDer::from_pem_file("server.crt")?;
    certs.add(cert)?;
    Ok(ClientConfig::with_root_certificates(Arc::new(certs))?)
}

async fn tunnel_listener(connection:Connection) -> Result<(), Box<dyn Error>>{
    loop {
        let (send, recv) = connection.accept_bi().await.unwrap();
        let stream = TcpStream::connect("127.0.0.1".to_owned()+":"+MC_PORT).await.unwrap();
        let (read, write) = stream.into_split();
        let buf = BytesMut::with_capacity(4096);
        tokio::spawn(async move {
            quic_to_tcp(recv, write).await;
        });
        tcp_to_quic(read, send, buf).await?;
    }
}

async fn handle_message_stream(connection:Connection) -> Result<(), Box<dyn Error>>{
    let (mut send, mut recv) = connection.open_bi().await.unwrap();
    let packet = create_packet(&[0;0], 0);
    send.write_chunk(packet.freeze()).await?;
    let mut data = BytesMut::with_capacity(4096);
    loop {
        if let Some(chunk) = recv.read_chunk(4096, true).await.unwrap(){
            data.put(chunk.bytes);
        }
        if let Some(packet) = get_packet(&data){
            if packet.id == 0 {
                let hostname = String::from_utf8(packet.payload.to_vec()).unwrap();
                println!("{}", hostname);
                data.advance(packet.size);
            }
        }
    }
    Ok(())
}

#[tokio::main()]
async fn main()-> Result<(), Box<dyn Error>> {
	let client_config = configure_client().unwrap();
	let mut endpoint = Endpoint::client(CLIENT_ADDR).unwrap();
    endpoint.set_default_client_config(client_config);
	let connection = endpoint.connect(SERVER_ADDR, SERVER_NAME).unwrap().await.unwrap();
    let connection2 = connection.clone();
    tokio::spawn(async move {
        tunnel_listener(connection2).await;
    });
    handle_message_stream(connection).await?;
    Ok(())
}