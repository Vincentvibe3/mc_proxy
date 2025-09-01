use std::{error::Error, io};

use bytes::Buf;
use mc_proxy_lib::packet::{create_packet, create_varint};
use rand::Rng;
use tokio::{io::AsyncWriteExt, net::TcpStream};

#[tokio::main()]
async fn main() -> Result<(), Box<dyn Error>> {
	let mut rng = rand::rng();
	let mut stream = TcpStream::connect("127.0.0.1:25565").await.unwrap();
	println!("connect");
	let mut data = Vec::with_capacity(10000000);
	for _ in 0..data.capacity() {
		data.push(rng.random::<u8>());
	}
	let subdomain = "mcsrv.vincentvibe3.com";
	let subdomain_bytes = subdomain.as_bytes();
	let mut proto_ver = create_varint(772);
	let mut string_size = create_varint(subdomain_bytes.len().try_into().unwrap());
	proto_ver.append(&mut string_size);
	proto_ver.append(&mut subdomain_bytes.to_vec());
	let mut handshake_packet = create_packet(&proto_ver, 0);
	loop {
		if handshake_packet.len() == 0 {
			break;
		}
		stream.writable().await?;
		// stream.write_all(data.as_slice()).await.unwrap();
		match stream.try_write(&handshake_packet) {
			Ok(0) => {
				break;
			},
			Ok(n) => {
				handshake_packet.advance(n);
			}
			Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                continue;
            }
            Err(e) => {
                return Err(e.into());
            }
		}
	}
	println!("generation done");
    loop {
        stream.writable().await?;
		match stream.try_write(data.as_slice()) {
			Ok(0) => {
				println!("closed");
				break;
			},
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