# OSTP System Architecture

## Overview
The Obfuscated Secure Transport Protocol (OSTP) is a high-performance, asynchronous network tunneling framework designed to provide secure, resilient, and indistinguishable data transport over untrusted networks. It is built entirely in Rust to guarantee memory safety, concurrency, and minimal overhead.

---

## Workspace Structure
The project is modularized into the following crates:
1. **ostp-core**: The core engine. Contains protocol state machines, Noise Protocol Framework handshakes, data framing serialization, dynamic obfuscation algorithms, and reliable packet delivery (ARQ).
2. **ostp-client**: The client daemon. Manages local traffic interception via dual-mode SOCKS5/HTTP proxies or virtualized network adapters (TUN/Wintun), multiplexing active host streams into a single UDP tunnel, and interfacing with TURN servers.
3. **ostp-server**: The high-concurrency connection dispatcher, responsible for demultiplexing data from multiple sessions, handling seamless IP roaming, and forwarding traffic to the broader internet.
4. **ostp-obfuscator**: Utility crate for static traffic shaping and dynamic obfuscation key derivation tools.
5. **ostp-jni**: Android JNI bindings that allow embedding OSTP inside mobile applications via an isolated runtime.
6. **ostp**: The unified command-line application that executes the protocol in either server or client mode.

---

## Framing Format and Data Structure

All multiplexed data is segmented into logical frames before encryption and transmission. The `FrameHeader` has a fixed size of **12 bytes**:

| Offset (Bytes) | Data Type | Field Name | Description |
| :--- | :--- | :--- | :--- |
| 0 | `u8` | `version` | Protocol version (current: `1`) |
| 1 | `u8` | `kind` | Frame type (see Kind Table below) |
| 2 | `u8` | `flags` | Control flags for stream management |
| 3 | `u8` | *reserved* | Reserved for future extensions (0) |
| 4-5 | `u16 BE` | `stream_id` | Logical identifier of the multiplexed stream |
| 6-9 | `u32 BE` | `payload_len` | Length of the actual payload in bytes |
| 10-11 | `u16 BE` | `pad_len` | Length of the appended adaptive padding |

### Frame Kinds (`FrameKind`):
- `1 - Handshake`: Key exchange payloads (Noise framework interaction).
- `2 - Data`: Encrypted upper-layer application payloads.
- `3 - Close`: Signals closure of a stream or the entire tunnel.
- `4 - KeepAlive`: Ping/Pong datagram to keep NAT mappings alive.
- `5 - Nack`: Explicit Negative Acknowledgment requesting immediate packet retransmission.
- `6 - Ack`: Confirms successful receipt of sequence number ranges.

A complete packet (`FramedPacket`) is encoded as:
`[12-byte FrameHeader]` + `[N-byte Payload]` + `[M-byte Padding]`

---

## Reliable ARQ System (Automatic Repeat reQuest)

To guarantee ordered, lossless data delivery over the unreliable UDP medium, `ostp-core` implements a custom Selective Repeat ARQ mechanism:

1. **Sequence Tracking**: Each data frame is assigned a strictly monotonic 64-bit `nonce`, which acts both as the sequence number and the initialization vector for the AEAD cipher.
2. **Transmission History (`sent_history`)**: Sent datagrams are cached until acknowledged by the peer. The buffer prevents memory bloat by enforcing a `max_sent_history` limit.
3. **Fast-Path Nack Retransmission**:
   - When a gap in the incoming sequence numbers is detected, the receiver immediately generates and transmits a `Nack` frame containing the missing sequence (`expected_recv_nonce`).
   - Upon receiving the `Nack`, the sender instantly locates the requested frame in its history and performs an immediate retransmission, bypassing standard timeout loops.
4. **Timeout-Based Retries (RTO / Tick)**:
   - A periodic `OstpEvent::Tick` fires every few milliseconds.
   - Any unacknowledged packet exceeding the Retransmission TimeOut (`rto_ms`) duration is retransmitted, incrementing its retry counter.
5. **Out-of-Order Delivery (`reorder_buffer`)**:
   - Packets received ahead of order are placed into a sorted B-Tree map.
   - Once the missing gap packets are successfully received, the buffer is flushed sequentially to deliver contiguous data to the application layer.

---

## Dynamic Roaming

OSTP is optimized for mobile environments. Session mappings are bound to unique cryptographic cryptographic identifiers (`session_id`), not network addresses.
When a client switches networks (e.g., transitioning from LTE to Wi-Fi):
1. Subsequent datagrams are sent from the new IP:Port, retaining the established `session_id` and cryptographic states.
2. The server decrypts the frame, identifies the session in its dispatcher registry, and instantly updates the return routing coordinates.
3. The user's active TCP streams within the tunnel remain alive and uninteruppted.
