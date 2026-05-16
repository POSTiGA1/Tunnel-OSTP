# 🌌 OSTP (Ospab Stealth Transport Protocol)

![GitHub Release](https://img.shields.io/github/v/release/ospab/ostp?style=flat-square&color=blue)
![License: BSL 1.1](https://img.shields.io/badge/License-BSL%201.1-orange.svg?style=flat-square)
![Platform: Windows | Linux | macOS | Android](https://img.shields.io/badge/Platform-Windows%20%7C%20Linux%20%7C%20macOS%20%7C%20Android-green.svg?style=flat-square)
![Rust: 1.75+](https://img.shields.io/badge/Rust-1.75%2B-red.svg?style=flat-square)

**OSTP** is a next-generation, high-performance stealth transport protocol engineered for absolute privacy and network resilience. It transforms your data streams into high-entropy, featureless noise, making it virtually undetectable by statistical network analysis (DPI).

Whether you are navigating restrictive network environments, securing industrial telemetry, or just seeking a robust personal tunnel, OSTP provides the stability and speed you need.

---

## ✨ Core Features

### 🛡️ Indistinguishable Traffic (Stealth)
Unlike traditional VPNs (OpenVPN, WireGuard) that have distinct packet signatures, OSTP uses advanced **Keystream Scrambling** and **Adaptive Block Shaping**. Your traffic looks like random bytes, bypassing even the most aggressive firewalls.

### 🚀 Extreme Performance
Written from the ground up in **Rust** and utilizing the **gVisor network stack**, OSTP is optimized for zero-copy data processing and high-throughput multiplexing. It easily handles 1Gbps+ streams with minimal CPU overhead.

### 📱 Cross-Platform Dominance
- **Windows**: Full support for TUN mode via Wintun and SOCKS5/HTTP proxying.
- **Linux**: Native high-performance daemon with systemd integration.
- **Android**: Integrated JNI core for mobile applications.
- **macOS/FreeBSD**: Standard CLI support for proxying and routing.

### 🔄 Intelligent Multiplexing (Mux)
Handle hundreds of concurrent streams over a single connection. OSTP includes a built-in Arq-based reliable transport layer that manages retransmissions and flow control automatically.

### 🏠 Robust Liveness (Keep-Alive)
Stays connected where others fail. The intelligent heartbeat system keeps NAT mappings alive and ensures the tunnel stays active even during long periods of idle time or network handoffs.

---

## 🛠️ Architecture

The project is organized into a modular workspace:
- **ostp-core**: The base cryptographic and framing library.
- **ostp-client**: High-level client logic, proxy servers, and TUN management.
- **ostp-server**: High-performance multi-tenant server implementation.
- **ostp**: The main CLI binary (The "Core").
- **ostp-jni**: Android/Mobile bindings.
- *Note: The experimental GUI is currently in a separate testing phase.*

---

## 📥 Getting Started

### 🐧 Linux (One-Line Installer)
```bash
bash <(curl -Ls https://raw.githubusercontent.com/ospab/ostp/master/scripts/install.sh)
```

### 🪟 Windows (One-Line Installer)
```powershell
# Run as Administrator
irm https://raw.githubusercontent.com/ospab/ostp/master/scripts/install.ps1 | iex
```

---

## ⚙️ Configuration

Generate your template first:
```bash
./ostp --init server # On VPS
./ostp --init client # On Local PC
```

### Server Example (`config.json`)
```json
{
  "_comment": "OSTP Server Configuration",
  "mode": "server",
  "listen": "0.0.0.0:50000",
  "access_keys": [
    "YOUR_GENERATED_KEY"
  ],
  "_comment_outbound": "Optional: forward traffic to another proxy (e.g. Tor)",
  "outbound": {
    "enabled": false,
    "protocol": "socks5",
    "address": "127.0.0.1",
    "port": 9050,
    "default_action": "proxy"
  }
}
```

### Client Example (`config.json`)
```json
{
  "_comment": "OSTP Client Configuration",
  "mode": "client",
  "server": "SERVER_IP:50000",
  "access_key": "YOUR_GENERATED_KEY",
  "socks5_bind": "127.0.0.1:1088",
  "tun": {
    "enable": false,
    "wintun_path": "./wintun.dll",
    "ipv4_address": "10.1.0.2/24",
    "dns": "1.1.1.1"
  }
}
```

---

## 📜 License & Legal

OSTP is published under the **Business Source License 1.1 (BSL)**. 
- **Personal/Private use**: Free and unrestricted.
- **Commercial use**: Requires a separate agreement until the change date.
- **Change Date**: May 14, 2030 (converts to **MIT License**).

See the [LICENSE](LICENSE) file for more details.
