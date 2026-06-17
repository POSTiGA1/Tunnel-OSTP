# OSTP Traffic Obfuscation

## Design Philosophy
Traditional tunneling protocols (such as TLS, OpenVPN, and WireGuard) exhibit distinct, recognizable fingerprints during key exchanges or carry static protocol headers. The OSTP obfuscation engine is explicitly designed to achieve **maximum entropy from the first byte**, rendering the transport completely indistinguishable from random, high-entropy noise to Deep Packet Inspection (DPI) systems.

---

## Obfuscation Key Derivation

To dynamically mask protocol data, an 8-byte obfuscation key is statically derived from the shared `access_key` configured on both the client and the server:

$$\text{Key} = \text{SHA-256}(\text{access\_key})[0..8]$$

This key is established pre-session and is never transmitted across the wire in any capacity.

---

## Dynamic In-Place Masking Algorithm

OSTP datagrams are processed "in-place" immediately prior to transmission and right after arrival. Two distinct mathematical modes are utilized based on the current handshake phase:

### 1. Handshake Phase Mode (`is_handshake = true`)
During connection initiation (Noise Handshake), the wire packet consists of a 4-byte `session_id` prefixed to the Noise payload. To mask the fixed session ID:

*   **Masking**: The first 4 bytes are XORed with the first 4 bytes of the derived obfuscation key:
    $$\text{raw}[i] = \text{raw}[i] \oplus \text{Key}[i \pmod 8], \quad i \in [0..3]$$
*   **De-masking**: A repeated XOR with the identical key bytes recovers the original `session_id`.

### 2. Data Transmission Mode (`is_handshake = false`)
Post-handshake, the wire layout contains:
`[4-byte session_id]` + `[8-byte nonce]` + `[AEAD Ciphertext]`

To completely randomize metadata, a two-tiered dynamic XOR masking process is applied:

1.  **Nonce Masking**: The 8-byte `nonce` (sequence counter) is XORed with the full 8-byte static key:
    $$\text{nonce\_bytes}[i] = \text{nonce\_bytes}[i] \oplus \text{Key}[i], \quad i \in [0..7]$$
2.  **Session ID Masking**: The 4-byte `session_id` is masked using high dynamic entropy — the lower 32 bits of the **original (unmasked)** `nonce` value:
    $$\text{session\_id\_bytes}[i] = \text{session\_id\_bytes}[i] \oplus \text{real\_nonce\_low32\_bytes}[i], \quad i \in [0..3]$$

#### Impact of the Scheme:
Because the `nonce` increments strictly with each outgoing datagram, the session ID's masking keystream continuously changes. This breaks all packet header correlations and eliminates repeating byte patterns, rendering statistical fingerprinting futile.

---

## Statistical Padding & Shaping

In addition to header obfuscation, OSTP defends against Traffic Length Analysis (TLA). 
The `AdaptivePadder` calculates dynamic dummy byte quantities to append to the packet payload before it enters the cryptographic step:

- **Dynamic Distributions**: The padding algorithms emulate length profiles commonly seen in whitelisted HTTPS or real-time video streams.
- **Encrypted Overheads**: The appended padding resides within the AEAD cipher scope. Consequently, passive observers cannot distinguish padding bytes from useful application payload, hiding the true message boundary lengths.

## XTLS-Reality Impersonation

OSTP provides a custom, dependency-free implementation of the XTLS-Reality protocol. It fully simulates a TLS 1.3 handshake (with realistic ClientHello profiles) to bypass advanced DPI filters. Post-handshake, it utilizes ChaCha20Poly1305 to seamlessly encrypt and tunnel the inner HTTP/WSS connections.

---

## Impossibility of Static Filtering (DPI Evasion)

Because of its strict mathematical entropy generation, the protocol is entirely devoid of plaintext signatures. This ensures that filtering systems (such as state censors like RKN or the Great Firewall) **physically cannot** write an effective blocking rule by analyzing packet contents. Any attempt to write a filter would inevitably result in blocking legitimate, randomized UDP traffic (like WebRTC or gaming traffic). Security is backed by Kerckhoffs's Principle — knowing the algorithms is useless for classifying traffic without possessing the `access_key`.