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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connect_roundtrip() {
        let msg = RelayMessage::Connect("example.com:443".to_string());
        let encoded = msg.encode();
        let decoded = RelayMessage::decode(&encoded).unwrap();
        match decoded {
            RelayMessage::Connect(addr) => assert_eq!(addr, "example.com:443"),
            _ => panic!("expected Connect"),
        }
    }

    #[test]
    fn test_data_roundtrip() {
        let data = vec![1, 2, 3, 4, 5];
        let msg = RelayMessage::Data(data.clone());
        let encoded = msg.encode();
        let decoded = RelayMessage::decode(&encoded).unwrap();
        match decoded {
            RelayMessage::Data(d) => assert_eq!(d, data),
            _ => panic!("expected Data"),
        }
    }

    #[test]
    fn test_simple_tags() {
        assert_eq!(RelayMessage::KeepAlive.encode(), vec![3]);
        assert_eq!(RelayMessage::Close.encode(), vec![4]);
        assert_eq!(RelayMessage::ConnectOk.encode(), vec![5]);

        assert!(matches!(RelayMessage::decode(&[3]).unwrap(), RelayMessage::KeepAlive));
        assert!(matches!(RelayMessage::decode(&[4]).unwrap(), RelayMessage::Close));
        assert!(matches!(RelayMessage::decode(&[5]).unwrap(), RelayMessage::ConnectOk));
    }

    #[test]
    fn test_error_roundtrip() {
        let msg = RelayMessage::Error("connection refused".to_string());
        let encoded = msg.encode();
        match RelayMessage::decode(&encoded).unwrap() {
            RelayMessage::Error(e) => assert_eq!(e, "connection refused"),
            _ => panic!("expected Error"),
        }
    }

    #[test]
    fn test_ping_pong_roundtrip() {
        let ts = 1234567890u64;
        match RelayMessage::decode(&RelayMessage::Ping(ts).encode()).unwrap() {
            RelayMessage::Ping(t) => assert_eq!(t, ts),
            _ => panic!("expected Ping"),
        }
        match RelayMessage::decode(&RelayMessage::Pong(ts).encode()).unwrap() {
            RelayMessage::Pong(t) => assert_eq!(t, ts),
            _ => panic!("expected Pong"),
        }
    }

    #[test]
    fn test_error_cases() {
        assert!(RelayMessage::decode(&[]).is_err());
        assert!(RelayMessage::decode(&[255]).is_err());
        // Truncated: tag=1, len=5, only 2 bytes
        assert!(RelayMessage::decode(&[1, 0, 5, b'a', b'b']).is_err());
    }

    #[test]
    fn test_empty_data_roundtrip() {
        let encoded = RelayMessage::Data(vec![]).encode();
        match RelayMessage::decode(&encoded).unwrap() {
            RelayMessage::Data(d) => assert!(d.is_empty()),
            _ => panic!("expected Data"),
        }
    }
}

