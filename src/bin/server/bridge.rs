use mc_proxy_lib::packet::create_packet;
use tokio::{io::{copy_bidirectional, AsyncWriteExt}, net::TcpStream};

pub async fn tunnel_streams(mut client: TcpStream, mut tunnel: TcpStream) {

    match copy_bidirectional(&mut client, &mut tunnel).await {
        Ok((n1, n2)) => {
			println!("{} {}: bidirectional", n2, n2);
		},
        Err(e) => eprintln!("An error occured proxying tunnel: {}", e),
    }
}