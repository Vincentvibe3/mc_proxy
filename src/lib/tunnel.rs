use std::error::Error;

use bytes::{Bytes, BytesMut};
use quinn::{RecvStream, SendStream};
use tokio::{io::{self, copy, copy_bidirectional, copy_buf, AsyncWriteExt, BufReader}, net::tcp::{OwnedReadHalf, OwnedWriteHalf}, sync::mpsc::{self, Receiver}, task::yield_now};

pub async fn quic_to_tcp(mut read:RecvStream, mut send:OwnedWriteHalf)-> Result<(), Box<dyn Error>>{
	// let mut buffer = BufReader::with_capacity(10000000, read);
	// copy_buf(&mut buffer, &mut send).await?;
    let (channel_send, channel_recv): (mpsc::Sender<Bytes>, Receiver<Bytes>) = mpsc::channel(500);
    tokio::spawn(async move {
        tcp_send(send, channel_recv).await;
    });
    loop {
		// println!("q read");
        let chunk_opt = read.read_chunk(4096, true).await.unwrap();
        if let Some(data) = chunk_opt {
            channel_send.send(data.bytes).await?;
            yield_now().await;
        }
    }
	Ok(())
}

async fn tcp_send(mut send:OwnedWriteHalf, mut channel:Receiver<Bytes>)-> Result<(), Box<dyn Error>>{
	'ext:loop {
		
		if let Some(value) = channel.recv().await {
            let mut bytes_written = 0;
            loop {
                send.writable().await?;
                match send.try_write(&value[bytes_written..]) {
                    Ok(0) => break 'ext,
                    Ok(n) => {
                        bytes_written+=n;
                        yield_now().await;
                    },
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        continue;
                    }
                    Err(e) => {
                        return Err(e.into());
                    }
                }
                if bytes_written == value.len() {
                    break;
                }
            }
			
			// send.write_all(&value).await?;
			// println!("t send");
		} else {
			break;
		}
	}
	Ok(())
}

async fn quic_send(mut send:SendStream, mut channel:Receiver<Bytes>)-> Result<(), Box<dyn Error>> {
    'ext:loop {
		
        if let Some(value) = channel.recv().await {
            let mut bytes_written = 0;
            loop {
                  match send.write(&value[bytes_written..]).await {
                    Ok(0) => break 'ext,
                    Ok(n) => {
                        bytes_written+=n;
                        yield_now().await;
                    },
                    Err(e) => {
                        return Err(e.into());
                    }
                }
                if bytes_written == value.len() {
                    break;
                }
            }
            // send.write_chunk(value).await?; 
			// println!("q send");
        } else {
            break;
        }
    }
    Ok(())
}

pub async fn tcp_to_quic(mut read:OwnedReadHalf, mut send:SendStream, mut data:BytesMut) -> Result<(), Box<dyn Error>>{
    let (channel_send, channel_recv): (mpsc::Sender<Bytes>, Receiver<Bytes>) = mpsc::channel(500);
    tokio::spawn(async move {
        quic_send(send, channel_recv).await;
    });
	// send.write_all(&data).await?;
	// let mut buffer = BufReader::with_capacity(10000000, read);
	// copy_buf(&mut buffer, &mut send).await?;
    loop {
		// println!("t read");
        read.readable().await?;
        if data.len() == data.capacity(){
            data.reserve(data.len()+1024);
        }
        match read.try_read_buf(&mut data) {
            Ok(0) => break,
            Ok(n) => {
                // println!("read {} bytes", n);
                yield_now().await;
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                // continue;
            }
            Err(e) => {
                return Err(e.into());
            }
        }
		if data.len() != 0 {
            channel_send.send(data.split().freeze()).await?;
        }
        
    }
    Ok(())
}