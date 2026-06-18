use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Cursor, Read, Write};

const BASE32_ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";

/// Encodes a byte slice into Base32 (RFC 4648) without padding, lowercase.
pub fn base32_encode(data: &[u8]) -> String {
    let mut result = String::with_capacity((data.len() * 8 + 4) / 5);
    let mut buffer = 0u32;
    let mut bits_left = 0;

    for &b in data {
        buffer = (buffer << 8) | (b as u32);
        bits_left += 8;
        while bits_left >= 5 {
            bits_left -= 5;
            let index = ((buffer >> bits_left) & 0x1F) as usize;
            result.push(BASE32_ALPHABET[index] as char);
        }
    }

    if bits_left > 0 {
        let index = ((buffer << (5 - bits_left)) & 0x1F) as usize;
        result.push(BASE32_ALPHABET[index] as char);
    }

    result
}

/// Decodes a Base32 string (case-insensitive, no padding) into a byte vector.
pub fn base32_decode(encoded: &str) -> Option<Vec<u8>> {
    let mut result = Vec::with_capacity(encoded.len() * 5 / 8);
    let mut buffer = 0u32;
    let mut bits_left = 0;

    for c in encoded.bytes() {
        let val = match c {
            b'a'..=b'z' => c - b'a',
            b'A'..=b'Z' => c - b'A',
            b'2'..=b'7' => c - b'2' + 26,
            _ => return None, // Invalid character
        };

        buffer = (buffer << 5) | (val as u32);
        bits_left += 5;

        if bits_left >= 8 {
            bits_left -= 8;
            result.push((buffer >> bits_left) as u8);
        }
    }

    Some(result)
}

#[derive(Debug, Clone, PartialEq)]
pub enum DnsRecordType {
    A,
    CNAME,
    NULL,
    TXT,
    AAAA,
    Unknown(u16),
}

impl From<u16> for DnsRecordType {
    fn from(val: u16) -> Self {
        match val {
            1 => DnsRecordType::A,
            5 => DnsRecordType::CNAME,
            10 => DnsRecordType::NULL,
            16 => DnsRecordType::TXT,
            28 => DnsRecordType::AAAA,
            _ => DnsRecordType::Unknown(val),
        }
    }
}

impl DnsRecordType {
    pub fn as_u16(&self) -> u16 {
        match self {
            DnsRecordType::A => 1,
            DnsRecordType::CNAME => 5,
            DnsRecordType::NULL => 10,
            DnsRecordType::TXT => 16,
            DnsRecordType::AAAA => 28,
            DnsRecordType::Unknown(v) => *v,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DnsQuestion {
    pub name: String,
    pub qtype: DnsRecordType,
    pub qclass: u16, // Usually 1 (IN)
}

#[derive(Debug, Clone)]
pub struct DnsAnswer {
    pub name: String,
    pub rtype: DnsRecordType,
    pub rclass: u16,
    pub ttl: u32,
    pub rdata: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct DnsPacket {
    pub id: u16,
    pub flags: u16,
    pub questions: Vec<DnsQuestion>,
    pub answers: Vec<DnsAnswer>,
}

impl DnsPacket {
    pub fn new_query(id: u16, name: &str, qtype: DnsRecordType) -> Self {
        DnsPacket {
            id,
            flags: 0x0100, // Standard query, recursion desired
            questions: vec![DnsQuestion {
                name: name.to_string(),
                qtype,
                qclass: 1, // IN
            }],
            answers: vec![],
        }
    }

    pub fn new_response(id: u16, name: &str, rtype: DnsRecordType, rdata: Vec<u8>) -> Self {
        DnsPacket {
            id,
            flags: 0x8180, // Response, standard query, recursion desired, recursion available
            questions: vec![DnsQuestion {
                name: name.to_string(),
                qtype: rtype.clone(),
                qclass: 1, // IN
            }],
            answers: vec![DnsAnswer {
                name: name.to_string(),
                rtype,
                rclass: 1,
                ttl: 0, // No caching
                rdata,
            }],
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        let _ = buf.write_u16::<BigEndian>(self.id);
        let _ = buf.write_u16::<BigEndian>(self.flags);
        let _ = buf.write_u16::<BigEndian>(self.questions.len() as u16);
        let _ = buf.write_u16::<BigEndian>(self.answers.len() as u16);
        let _ = buf.write_u16::<BigEndian>(0); // Authority PR
        let _ = buf.write_u16::<BigEndian>(0); // Additional PR

        for q in &self.questions {
            encode_domain_name(&mut buf, &q.name);
            let _ = buf.write_u16::<BigEndian>(q.qtype.as_u16());
            let _ = buf.write_u16::<BigEndian>(q.qclass);
        }

        for a in &self.answers {
            encode_domain_name(&mut buf, &a.name);
            let _ = buf.write_u16::<BigEndian>(a.rtype.as_u16());
            let _ = buf.write_u16::<BigEndian>(a.rclass);
            let _ = buf.write_u32::<BigEndian>(a.ttl);
            
            if a.rtype == DnsRecordType::TXT {
                // TXT records have character-strings length-prefixed
                // We split into chunks of up to 255 bytes
                let mut txt_data = Vec::new();
                for chunk in a.rdata.chunks(255) {
                    txt_data.push(chunk.len() as u8);
                    txt_data.extend_from_slice(chunk);
                }
                let _ = buf.write_u16::<BigEndian>(txt_data.len() as u16);
                buf.extend_from_slice(&txt_data);
            } else {
                let _ = buf.write_u16::<BigEndian>(a.rdata.len() as u16);
                buf.extend_from_slice(&a.rdata);
            }
        }

        buf
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() < 12 {
            return None;
        }

        let mut cursor = Cursor::new(data);
        let id = cursor.read_u16::<BigEndian>().ok()?;
        let flags = cursor.read_u16::<BigEndian>().ok()?;
        let qdcount = cursor.read_u16::<BigEndian>().ok()?;
        let ancount = cursor.read_u16::<BigEndian>().ok()?;
        let _nscount = cursor.read_u16::<BigEndian>().ok()?;
        let _arcount = cursor.read_u16::<BigEndian>().ok()?;

        let mut questions = Vec::new();
        for _ in 0..qdcount {
            let name = decode_domain_name(&mut cursor, data)?;
            let qtype = cursor.read_u16::<BigEndian>().ok()?.into();
            let qclass = cursor.read_u16::<BigEndian>().ok()?;
            questions.push(DnsQuestion { name, qtype, qclass });
        }

        let mut answers = Vec::new();
        for _ in 0..ancount {
            let name = decode_domain_name(&mut cursor, data)?;
            let rtype: DnsRecordType = cursor.read_u16::<BigEndian>().ok()?.into();
            let rclass = cursor.read_u16::<BigEndian>().ok()?;
            let ttl = cursor.read_u32::<BigEndian>().ok()?;
            let rdlength = cursor.read_u16::<BigEndian>().ok()?;

            let mut rdata = vec![0u8; rdlength as usize];
            cursor.read_exact(&mut rdata).ok()?;

            if rtype == DnsRecordType::TXT {
                // Decode TXT string chunks
                let mut decoded_txt = Vec::new();
                let mut txt_cursor = Cursor::new(&rdata);
                while txt_cursor.position() < rdata.len() as u64 {
                    if let Ok(len) = txt_cursor.read_u8() {
                        let mut chunk = vec![0u8; len as usize];
                        if txt_cursor.read_exact(&mut chunk).is_ok() {
                            decoded_txt.extend_from_slice(&chunk);
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                rdata = decoded_txt;
            }

            answers.push(DnsAnswer {
                name,
                rtype,
                rclass,
                ttl,
                rdata,
            });
        }

        // Skip authority and additional sections (not needed for basic payload extraction)
        
        Some(DnsPacket {
            id,
            flags,
            questions,
            answers,
        })
    }
}

fn encode_domain_name(buf: &mut Vec<u8>, name: &str) {
    for part in name.split('.') {
        if part.is_empty() {
            continue;
        }
        let len = part.len().min(63) as u8;
        buf.push(len);
        buf.extend_from_slice(&part.as_bytes()[..len as usize]);
    }
    buf.push(0); // Root label
}

fn decode_domain_name(cursor: &mut Cursor<&[u8]>, full_data: &[u8]) -> Option<String> {
    let mut parts = Vec::new();
    let mut jumps = 0;
    let mut current_pos = cursor.position();

    loop {
        if jumps > 100 {
            return None; // Prevent infinite loops
        }
        
        if current_pos >= full_data.len() as u64 {
            return None;
        }
        
        let len = full_data[current_pos as usize];
        if len == 0 {
            if jumps == 0 {
                cursor.set_position(current_pos + 1);
            }
            break;
        }

        if len & 0xC0 == 0xC0 {
            // Pointer
            if current_pos + 1 >= full_data.len() as u64 {
                return None;
            }
            let pointer = (((len & 0x3F) as u16) << 8) | (full_data[current_pos as usize + 1] as u16);
            if jumps == 0 {
                cursor.set_position(current_pos + 2);
            }
            jumps += 1;
            current_pos = pointer as u64;
            continue;
        }

        current_pos += 1;
        if current_pos + len as u64 > full_data.len() as u64 {
            return None;
        }
        
        let part = &full_data[current_pos as usize..(current_pos + len as u64) as usize];
        parts.push(String::from_utf8_lossy(part).into_owned());
        current_pos += len as u64;

        if jumps == 0 {
            cursor.set_position(current_pos);
        }
    }

    if parts.is_empty() {
        Some(".".to_string())
    } else {
        Some(parts.join("."))
    }
}

/// Encodes a payload into a list of subdomain labels and appends the base domain.
/// Each label is max 63 chars. The base32 string is chunked.
pub fn encode_payload_to_domain(payload: &[u8], base_domain: &str) -> String {
    let encoded = base32_encode(payload);
    let mut domain = String::new();
    
    let mut start = 0;
    while start < encoded.len() {
        let end = (start + 63).min(encoded.len());
        domain.push_str(&encoded[start..end]);
        domain.push('.');
        start = end;
    }
    
    domain.push_str(base_domain);
    domain
}

/// Decodes a payload from a subdomain string, ignoring the base domain.
pub fn decode_domain_to_payload(full_domain: &str, base_domain: &str) -> Option<Vec<u8>> {
    // Strip base domain and trailing dots
    let stripped = full_domain
        .trim_end_matches('.')
        .strip_suffix(base_domain)?;
        
    let stripped = stripped.trim_end_matches('.');
    
    let mut base32_str = String::with_capacity(stripped.len());
    for part in stripped.split('.') {
        base32_str.push_str(part);
    }
    
    base32_decode(&base32_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base32() {
        let data = b"Hello, OSTP DNS Tunnel!";
        let encoded = base32_encode(data);
        let decoded = base32_decode(&encoded).unwrap();
        assert_eq!(data.as_ref(), decoded.as_slice());
    }

    #[test]
    fn test_domain_encoding() {
        let payload = vec![0x12; 20];
        let base_domain = "tunnel.example.com";
        let domain = encode_payload_to_domain(&payload, base_domain);
        
        // Ensure no label is > 63 chars
        for part in domain.split('.') {
            assert!(part.len() <= 63);
        }
        
        assert!(domain.ends_with(base_domain));
        
        let decoded = decode_domain_to_payload(&domain, base_domain).unwrap();
        assert_eq!(payload, decoded);
    }
    
    #[test]
    fn test_dns_packet() {
        let payload = vec![1, 2, 3, 4, 5];
        let domain = encode_payload_to_domain(&payload, "t.com");
        
        let query = DnsPacket::new_query(1234, &domain, DnsRecordType::TXT);
        let encoded_query = query.encode();
        
        let decoded_query = DnsPacket::decode(&encoded_query).unwrap();
        assert_eq!(decoded_query.id, 1234);
        assert_eq!(decoded_query.questions[0].name, domain);
        assert_eq!(decoded_query.questions[0].qtype, DnsRecordType::TXT);
        
        let response_data = vec![5, 4, 3, 2, 1];
        let response = DnsPacket::new_response(1234, &domain, DnsRecordType::TXT, response_data.clone());
        let encoded_resp = response.encode();
        
        let decoded_resp = DnsPacket::decode(&encoded_resp).unwrap();
        assert_eq!(decoded_resp.answers[0].rdata, response_data);
    }
}
