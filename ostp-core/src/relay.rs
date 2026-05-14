use anyhow::{anyhow, Result};

#[derive(Debug, Clone)]
pub enum RelayMessage {
    Connect(String),
    Data(Vec<u8>),
    KeepAlive,
    Close,
    ConnectOk,
    Error(String),
    Ping(u64),
    Pong(u64),
}

impl RelayMessage {
    pub fn encode(&self) -> Vec<u8> {
        match self {
            RelayMessage::Connect(addr) => encode_with_len(1, addr.as_bytes()),
            RelayMessage::Data(data) => encode_with_len(2, data),
            RelayMessage::KeepAlive => vec![3],
            RelayMessage::Close => vec![4],
            RelayMessage::ConnectOk => vec![5],
            RelayMessage::Error(msg) => encode_with_len(6, msg.as_bytes()),
            RelayMessage::Ping(ts) => encode_with_len(7, &ts.to_be_bytes()),
            RelayMessage::Pong(ts) => encode_with_len(8, &ts.to_be_bytes()),
        }
    }

    pub fn decode(input: &[u8]) -> Result<Self> {
        if input.is_empty() {
            return Err(anyhow!("empty relay message"));
        }

        match input[0] {
            1 => {
                let payload = decode_with_len(&input[1..])?;
                let addr = String::from_utf8(payload.to_vec())
                    .map_err(|_| anyhow!("invalid utf8 in connect addr"))?;
                Ok(RelayMessage::Connect(addr))
            }
            2 => Ok(RelayMessage::Data(decode_with_len(&input[1..])?.to_vec())),
            3 => Ok(RelayMessage::KeepAlive),
            4 => Ok(RelayMessage::Close),
            5 => Ok(RelayMessage::ConnectOk),
            6 => {
                let payload = decode_with_len(&input[1..])?;
                let msg = String::from_utf8(payload.to_vec())
                    .map_err(|_| anyhow!("invalid utf8 in error message"))?;
                Ok(RelayMessage::Error(msg))
            }
            7 => {
                let payload = decode_with_len(&input[1..])?;
                if payload.len() != 8 { return Err(anyhow!("invalid ping payload len")); }
                let ts = u64::from_be_bytes(payload.try_into().unwrap());
                Ok(RelayMessage::Ping(ts))
            }
            8 => {
                let payload = decode_with_len(&input[1..])?;
                if payload.len() != 8 { return Err(anyhow!("invalid pong payload len")); }
                let ts = u64::from_be_bytes(payload.try_into().unwrap());
                Ok(RelayMessage::Pong(ts))
            }
            t => Err(anyhow!("unknown relay message type {t}")),
        }
    }
}

fn encode_with_len(tag: u8, payload: &[u8]) -> Vec<u8> {
    let len = payload.len().min(u16::MAX as usize) as u16;
    let mut out = Vec::with_capacity(1 + 2 + len as usize);
    out.push(tag);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&payload[..len as usize]);
    out
}

fn decode_with_len(input: &[u8]) -> Result<&[u8]> {
    if input.len() < 2 {
        return Err(anyhow!("relay payload length prefix missing"));
    }
    let len = u16::from_be_bytes([input[0], input[1]]) as usize;
    if input.len() < 2 + len {
        return Err(anyhow!("relay payload truncated"));
    }
    Ok(&input[2..2 + len])
}
