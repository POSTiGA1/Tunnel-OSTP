# OSTP Server Daemon

## Overview
The OSTP Server functions as a high-performance network gateway, engineered to concurrently serve thousands of anonymous, obfuscated secure tunnels. It handles raw packet demultiplexing, decrypts encapsulated payloads, and proxies standard stream traffic out to the destination internet endpoints.

---

## Dispatcher Core Architecture

The core scheduler of the server is the centralized `Dispatcher` module. Departing from traditional synchronous, thread-per-socket designs, it enforces a strict separation of network I/O and session states:

1. **Asynchronous Socket Poll**: An independent asynchronous ingestion task continuously reads datagrams from the global `UdpSocket` and channels them directly to the multi-threaded dispatcher dispatch queue.
2. **Crypto Session Registry**: The dispatcher maintains an efficient hash map containing all active client states, indexed by their `session_id`.
3. **Zero-Copy Routing**: For every incoming payload, the dispatcher executes a fast `O(1)` state matching query. Once a valid `session_id` registry matches, the ciphertext is passed directly to its dedicated `ProtocolMachine` for execution.

---

## Attack Mitigation & Intrusion Resilience

Because public endpoints are exposed to continuous probe traffic and Denial-of-Service (DoS) attempts, the server implements multiple confinement layers:

### 1. Isolated Packet Rejection
Any corrupted frame, AEAD authentication tag failure, or malformed protocol packet instantly terminates in a silent packet drop event:
- Processing faults are localized immediately during the initial extraction block.
- Existing, authenticated sessions **are never terminated or reset** when an invalid packet arrives on their matching ID. This strictly blocks blind packet injection (spoofing) vectors aimed at interrupting existing user tunnels.

### 2. Replay Prevention
To defend against man-in-the-middle adversaries intercepting and later replaying valid UDP handshake frames:
- Client handshakes embed cryptographic chronological markers (timestamps) in their payload envelopes.
- The server validates timestamps against local system clocks, rejecting attempts outside acceptable synchronization limits.
- Accepted handshake material is cached temporarily in a memory cache to categorically discard exact bitwise re-transmissions.

---

## Zero-Latency Client Roaming

The server inherently treats IP:Port coordinates as fluid and volatile variables rather than static identifiers:
- Upon receiving **any successfully decrypted and authenticated** data frame, the dispatcher reads its immediate source IP and port.
- If this origin deviates from the recorded tracking coordinate for that session, the server executes an atomic in-place update.
- Subsequent outbound packets designated for the client are instantly dispatched to the newly updated endpoint.
- This methodology facilitates millisecond-level handoffs during cellular tower changes or Wi-Fi switches, fully preserving upper TCP sessions.
