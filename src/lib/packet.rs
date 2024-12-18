use std::i32;

use bytes::{BufMut, Bytes, BytesMut};
use uuid::Uuid;

#[derive(Debug)]
pub struct Packet<'a> {
    pub size: usize,
    pub id: i32,
    pub payload: &'a [u8]
}

pub struct ForwardedPayload {
    pub uuid:Uuid,
    pub payload:Bytes
}

pub fn get_packet<'a>(data:&'a BytesMut) -> Option<Packet<'a>>{
    let mut total_bytes_read = 0;
    let (packet_size, bytes_read) = read_varint(data);
    let varint_size = bytes_read;
    let packet_usize = usize::try_from(packet_size).unwrap();
    if data.len()-bytes_read < packet_usize {
        return None;
    }
    total_bytes_read+=bytes_read;
    let (id, bytes_read) = read_varint(&data[total_bytes_read..]);
    total_bytes_read+=bytes_read;
    let payload = data.get(total_bytes_read..packet_usize+varint_size).unwrap();
    if total_bytes_read == 0 {
        return None;
    }
    return Some(Packet {
        size: packet_usize+varint_size,
        id: id,
        payload: payload
    });
}

pub fn get_forwarded_payload(data:&Bytes) -> ForwardedPayload{
    let uuid_bytes = data[..16].to_vec();
    return ForwardedPayload {
        uuid: Uuid::from_slice(&uuid_bytes).unwrap(),
        payload: data.slice(16..)
    };
}

pub fn add_size_to_data(buffer: &[u8]) -> Vec<u8>{
    let size = buffer.len();
    let mut varint = create_varint(i32::try_from(size).unwrap());
    varint.extend_from_slice(buffer);
    return varint;
}

pub fn create_packet(buffer: &[u8], id:i32) -> BytesMut {
    let id_varint = create_varint(i32::try_from(id).unwrap());
    let id_varint_len = id_varint.len();
    let payload_len = buffer.len();
    let size_bytes = create_varint(i32::try_from(payload_len+id_varint_len).unwrap());
    let mut packet = BytesMut::with_capacity(size_bytes.len()+payload_len+id_varint_len);
    packet.put_slice(&size_bytes);
    packet.put_slice(&id_varint);
    packet.put_slice(buffer);
    return packet;
}


/* Packet format

1. Varint for packet size
2. Packet type id (varint)
3. Packet Uuid bytes (16 bytes)
5. Payload

*/
pub fn create_forwarded_packet(buffer: &[u8], uuid:Uuid, id:i32) -> BytesMut {
    let mut uuid_bytes = uuid.as_bytes().to_vec();
    uuid_bytes.extend_from_slice(&buffer);
    return create_packet(&uuid_bytes, id);
}

pub fn create_varint(int:i32) -> Vec<u8>{
    let mut value = int;
    let mut bytes = Vec::<u8>::new();
    const CONTINUE_BIT:i32 = 0x80;
    const DATA_BITS:i32 = 0x7F;
    loop {
        if value & !DATA_BITS == 0 {
            let u8_value = u8::try_from(value).unwrap();
            bytes.push(u8_value);
            break;
        }
        let u8_value = u8::try_from(value & DATA_BITS | CONTINUE_BIT).unwrap();
        bytes.push(u8_value);
        value >>= 7;
    }
    return bytes;
}

pub fn read_varint(buffer: &[u8]) -> (i32, usize){
    const CONTINUE_BIT:u8 = 0x80;
    const DATA_BITS:u8 = 0x7F;
    let mut value:i32 = 0;
    let mut position = 0;
    let mut bytes_read = 0;
    for byte in buffer{
        let data = i32::from(byte & DATA_BITS) << position;
        value |= data;
        bytes_read+=1;
        if byte & CONTINUE_BIT == 0{
            break;
        }
        position+=7;
        if position >= 32 {
            panic!("VarInt is too big")
        }
    }
    return (value, bytes_read);
}

pub fn read_string(buffer: &[u8]) -> (String, usize) {
    let (len, bytes_read) = read_varint(buffer);
    let len_usize = usize::try_from(len).unwrap();
    println!("string len {len}");
    return (String::from_utf8(buffer[bytes_read..(bytes_read+len_usize)].to_vec()).unwrap(), bytes_read+len_usize);
}