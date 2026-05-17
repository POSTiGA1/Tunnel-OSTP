# OSTP — Ospab Stealth Transport Protocol

[Русский язык](README.ru.md)

![GitHub Release](https://img.shields.io/github/v/release/ospab/ostp?style=flat-square&color=blue)
![License: BSL 1.1](https://img.shields.io/badge/License-BSL%201.1-orange.svg?style=flat-square)
![Platform: Windows | Linux | macOS | Android](https://img.shields.io/badge/Platform-Windows%20%7C%20Linux%20%7C%20macOS%20%7C%20Android-green.svg?style=flat-square)

OSTP is a high-performance, censorship-resistant transport protocol designed to tunnel TCP traffic over UDP with full traffic obfuscation. It is resistant to Deep Packet Inspection (DPI), active probing, and statistical traffic analysis.

---

## Key Features

| Feature | Description |
|---------|-------------|
| **Traffic Obfuscation** | Every packet — including headers — is indistinguishable from random noise on the wire. Session IDs and nonces are masked with per-packet HMAC-derived keys. |
| **Noise Protocol Handshake** | `Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s` — pre-shared key authenticated, forward-secret key exchange with no static identity exposure. |
| **Reliable UDP (ARQ)** | Selective ACK/NACK with rate-limited retransmission, configurable reorder buffer, and exponential backoff. Designed for 10 Gbps throughput. |
| **Multiplexed Streams** | Multiple logical TCP streams over a single encrypted UDP session, with per-stream flow control. |
| **Seamless Roaming** | Clients can switch networks (WiFi ↔ 4G) without session interruption — the server tracks session-ID, not IP address. |
| **TUN Mode** | Full-system VPN via `tun2socks` integration on Windows and Linux. All traffic is transparently routed through the tunnel. |
| **TURN Relay** | RFC 5766 TURN support for environments where direct UDP is blocked. |
| **Hot-Reload** | Runtime config reload without restarting the process (access keys, exclusions, mux settings, TURN). |
| **Cross-Platform** | Windows, Linux, macOS, Android. Single binary, no runtime dependencies. |

---

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│  Client                                                     │
│  ┌──────────┐   ┌──────────┐   ┌────────────────────────┐   │
│  │ Browser  │──▸│ SOCKS5/  │──▸│    Bridge (Mux)        │   │
│  │ / Apps   │   │ HTTP     │   │  ┌─────────────────┐   │   │
│  │          │   │ Proxy    │   │  │ ProtocolMachine │   │   │
│  └──────────┘   └──────────┘   │  │ (Noise + AEAD)  │   │   │
│                                │  └────────┬────────┘   │   │
│  ┌──────────┐                  │           │            │   │
│  │ TUN Mode │──────────────────┤      UDP Socket        │   │
│  │tun2socks │                  │  (32MB buffers,        │   │
│  └──────────┘                  │   obfuscated wire)     │   │
│                                └───────────┬────────────┘   │
└────────────────────────────────────────────┼────────────────┘
                                             │ UDP
┌────────────────────────────────────────────┼────────────────┐
│  Server                                    │                │
│  ┌─────────────────────────────────────────┴───────────┐    │
│  │              Dispatcher                             │    │
│  │  (Session lookup, roaming detection, replay guard)  │    │
│  └──────────────┬──────────────────────────────────────┘    │
│                 │                                           │
│  ┌──────────────▾──────────────────┐                        │
│  │   Relay Loop (per-stream TCP)   │──▸ Internet / Backend  │
│  └─────────────────────────────────┘                        │
└─────────────────────────────────────────────────────────────┘
```

---

## Installation

### Linux
```bash
bash <(curl -Ls https://raw.githubusercontent.com/ospab/ostp/master/scripts/install.sh)
```

### Windows (PowerShell, Administrator)
```powershell
irm https://raw.githubusercontent.com/ospab/ostp/master/scripts/install.ps1 | iex
```

---

## Configuration

Generate a default config:
```bash
./ostp --init server   # VPS
./ostp --init client   # Local machine
```

### Server (`config.json`)
```jsonc
{
  "mode": "server",
  "listen": "0.0.0.0:50000",
  "access_keys": ["YOUR_SECRET_KEY"],
  "debug": false,
  // Optional: forward traffic through an upstream proxy
  "outbound": {
    "enabled": false,
    "protocol": "socks5",    // "socks5" or "http"
    "address": "127.0.0.1",
    "port": 9050,
    "default_action": "proxy"
  }
}
```

### Client (`config.json`)
```jsonc
{
  "mode": "client",
  "server": "YOUR_SERVER_IP:50000",
  "access_key": "YOUR_SECRET_KEY",
  "socks5_bind": "127.0.0.1:1088",
  "debug": false,
  // TUN mode (full-system VPN)
  "tun": {
    "enable": false,
    "dns": "1.1.1.1"
  },
  // Multiplexing: spread traffic across multiple UDP sessions
  "mux": {
    "enabled": false,
    "sessions": 2
  },
  // TURN relay for restricted networks
  "turn": {
    "enabled": false,
    "server_addr": "turn.example.com:3478",
    "username": "user",
    "access_key": "pass"
  },
  // Traffic exclusions (bypassed directly)
  "exclude": {
    "domains": ["example.local"],
    "ips": ["192.168.0.0/16"]
  }
}
```

---

## Usage

```bash
# Start with config
./ostp --config config.json

# Or just run (looks for config.json in current/binary directory)
./ostp
```

### TUN Mode (Windows)
Requires `tun2socks.exe` in the same directory. Automatically requests Administrator privileges.

### TUN Mode (Linux)
Requires root. Uses `tun2socks` binary (same directory or in `$PATH`).

---

## Protocol Specification

See [docs/en/specification.md](docs/en/specification.md) for the full wire format, handshake flow, and ARQ semantics.

### Quick Summary

| Layer | Mechanism |
|-------|-----------|
| Key Exchange | Noise NNpsk0 (X25519 + ChaChaPoly + BLAKE2s) |
| Encryption | ChaCha20-Poly1305 AEAD per-packet |
| Header Obfuscation | HMAC-SHA256 derived per-packet mask over session_id + nonce |
| Reliability | Selective ACK with cumulative + SACK ranges |
| Retransmission | Rate-limited NACK (30ms cooldown) + exponential backoff RTO |
| Flow Control | In-flight window (retransmittable frames only) |
| Keepalive | Ping/Pong with RTT measurement every 5s |
| Session Timeout | 60s inactivity on client, 300s on server |

---

## Building from Source

```bash
# Prerequisites: Rust toolchain (1.75+)
cargo build --release

# Cross-compile for Linux (from Windows/macOS)
cross build --release --target x86_64-unknown-linux-gnu
```

---

## Documentation

- [Architecture Overview](docs/en/architecture.md)
- [Protocol Specification](docs/en/specification.md)
- [Obfuscation Design](docs/en/obfuscation.md)
- [Server Administration](docs/en/server.md)
- [Client Configuration](docs/en/client.md)
- [Integration Guide](docs/en/integrations.md)

---

## License

Business Source License 1.1. Free for personal and non-commercial use.  
Converts to MIT License on May 14, 2030.
