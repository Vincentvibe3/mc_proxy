use std::error::Error;

use bytes::BytesMut;
use tokio::{io, net::TcpStream, task::yield_now};


async fn handle_connection(stream:TcpStream, conn_num:i32)-> Result<(), Box<dyn Error>>{
    let mut data = BytesMut::with_capacity(10000);
	loop {
		stream.readable().await?;
		match stream.try_read_buf(&mut data) {
			Ok(0) => {
				println!("closed");
				break;
			},
			Ok(n) => {
				// continue;
				yield_now().await;
			}
			Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
				println!("would block {}", conn_num);
                continue;
            }
            Err(e) => {
                return Err(e.into());
            }
		}
		data.clear();
	}
    Ok(())
}

#[tokio::main()]
async fn main(){
    let listener = tokio::net::TcpListener::bind("0.0.0.0:25565").await.unwrap();
	let mut conn_num = 0;
    loop {
        let socket = listener.accept().await.unwrap();
		println!("accepting");
		conn_num+=1;
        tokio::spawn(async move {
            let stream = socket.0;
            handle_connection(stream, conn_num).await.unwrap();
        });

    }
}