use ostp_core::{ProtocolMachine, ProtocolConfig, OstpEvent, ProtocolAction, NoiseRole};

fn main() {
    let key = "3f5dfaf68e377a3724bdde3ac7b4f4de";
    let secrets = ostp_core::crypto::derive_all_secrets(key.as_bytes());
    
    let mut init_cfg = ProtocolConfig {
        role: NoiseRole::Initiator,
        session_id: 12345,
        psk: secrets.psk,
        obfuscation_key: secrets.obfuscation_key,
        handshake_pad_min: secrets.handshake_pad_min,
        handshake_pad_max: secrets.handshake_pad_max,
        max_reorder: 10,
        max_reorder_buffer: 10,
        ack_delay_ms: 10,
        rto_ms: 100,
        max_retries: 5,
        max_sent_history: 100,
        handshake_payload: vec![],
        mtu: 1400,
        max_padding: 0,
        padding_strategy: ostp_core::PaddingStrategy::Adaptive,
    };
    
    let mut payload = Vec::new();
    payload.extend_from_slice(&0u64.to_be_bytes()); // time
    payload.extend_from_slice(&12345u32.to_be_bytes());
    payload.extend_from_slice(key.as_bytes());
    init_cfg.handshake_payload = payload;

    let mut init_machine = ProtocolMachine::new(init_cfg.clone()).unwrap();
    let action = init_machine.on_event(OstpEvent::Start).unwrap();
    
    let pkt = match action {
        ProtocolAction::SendDatagram(p) => p,
        _ => panic!("Expected SendDatagram"),
    };
    
    println!("Initiator sent {} bytes", pkt.len());
    
    let mut resp_cfg = init_cfg.clone();
    resp_cfg.role = NoiseRole::Responder;
    
    let mut resp_machine = ProtocolMachine::new(resp_cfg).unwrap();
    
    // Simulate what server dispatcher does
    let mut raw_vec = pkt.to_vec();
    ostp_core::crypto::deobfuscate_packet_inplace(&mut raw_vec, &secrets.obfuscation_key, true);
    println!("Deobfuscated length: {}", raw_vec.len());
    
    let action = resp_machine.on_event(OstpEvent::Inbound(pkt));
    match action {
        Ok(ProtocolAction::HandshakePayload(_, _)) => println!("Responder: Handshake OK!"),
        Ok(_) => println!("Responder: Not HandshakePayload"),
        Err(e) => println!("Responder error: {:?}", e),
    }
}
