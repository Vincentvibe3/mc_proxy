use std::{error::Error, io};

use mc_proxy_lib::packet::{create_packet, create_varint};
use rand::Rng;
use tokio::{io::AsyncWriteExt, net::TcpStream};

#[tokio::main()]
async fn main() -> Result<(), Box<dyn Error>> {
	let mut rng = rand::rng();
	let mut stream = TcpStream::connect("mcsrv.vincentvibe3.com:25565").await.unwrap();
	let subdomain = "mcsrv.vincentvibe3.com";
	let subdomain_bytes = subdomain.as_bytes();
	let mut proto_ver = create_varint(772);
	proto_ver.append(&mut subdomain_bytes.to_vec());
	let handshake_packet = create_packet(&proto_ver, 0);
	stream.write_all(&handshake_packet).await?;
	let mut data = Vec::with_capacity(10000000);
	for _ in 0..data.len() {
		data.push(rng.random::<u8>());
	}
    loop {
        stream.writable().await?;
		match stream.try_write(data.as_slice()) {
			Ok(0) => break,
			Ok(n) => {
				continue;
			}
			Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                continue;
            }
            Err(e) => {
                return Err(e.into());
            }
		}

    }
    Ok(())
}