# OSTP Client Daemon

## Overview
The OSTP Client operates as an autonomous background daemon (or system service) responsible for high-performance interception of local application traffic, encapsulation into the obfuscated secure tunnel, and maintaining robust endpoint connectivity to the remote OSTP server.

---

## Traffic Ingestion Mechanisms

To maximize platform compatibility and application support, the client integrates three primary mechanisms:

### 1. Dual-Protocol Inbound Proxy
The internal proxy server binds to a single TCP port and dynamically distinguishes the protocol based on the initial byte of the incoming stream:
- **SOCKS5 (RFC 1928)**: Activated when the first byte equals `0x05`. Standard stream encapsulation occurs.
- **HTTP Forward Proxy**: Triggered when the first byte differs from `0x05`. The parser supports:
  - The `CONNECT host:port` method for establishing encrypted end-to-end TLS pipelines.
  - Standard `GET http://...` methods for clear-text HTTP proxying.

### 2. Windows System Proxy Integration (Sysproxy)
For zero-configuration deployments on Windows, the client programmatically configures the host's system proxy configuration (WinINet API):
- Proxy server registries are written in the strict format demanded by modern browsers (Edge, Chrome, Firefox):
  `http=127.0.0.1:1088;https=127.0.0.1:1088`
- Upon graceful shutdown, previous registry values are fully restored, ensuring the user is never left without basic internet connectivity.

### 3. Virtual Network Interface (TUN/Wintun)
On Windows and Linux, the client can instantiate a high-speed virtual TUN adapter (utilizing the **Wintun** driver):
- Intercepts 100% of machine traffic at OSI Layer 3 (raw IP packets).
- A lightweight internal user-space TCP/IP stack synthetically reconstructs logical streams and routes them into the OSTP multiplexer, enabling system-wide VPN-grade tunneling without manual application configurations.

---

## NAT Traversal and Port-Aligned Discovery

Successfully routing UDP traffic past carrier-grade firewalls (Symmetric and Port-Restricted NATs) requires deterministic port handling:
1. **Unified Socket**: The client binds exactly *one* underlying `UdpSocket`.
2. **STUN/TURN Discovery**: Utilizing the active socket, it issues STUN queries or orchestrates authenticated TURN allocations (RFC 5766) via pure-Rust `HMAC-SHA1` and `MD5` hashing logic.
3. **Mapping Reuse**: Following NAT coordinate identification, all subsequent OSTP payload transmissions utilize **the same primary socket**. Edge routers treat this as a single persistent egress flow, allowing the remote server's incoming packets to bypass firewall blocks.

---

## Fault Tolerance & Automated Recovery

The client is engineered to maintain persistence without requiring user intervention:
- **Infinite Reconnection Loop**: When the orchestration loop (`runner.rs`) captures a `UiEvent::TunnelStopped`, it automatically schedules a tunnel restart after a fixed 5-second back-off. This loop contains no maximum attempt caps, pursuing restoration until the user issues a termination command.
- **Log De-noising**: Standard, expected TCP interruptions (such as `ConnectionReset`, `BrokenPipe`, or `UnexpectedEof`) are actively suppressed from console output, preserving log clarity for true state transitions (`Idle -> Connecting -> Connected`).

---

## Modular Routing Architecture (Inbounds / Outbounds)

Starting from version `0.3.1`, the OSTP client utilizes a modular configuration architecture based on inbound and outbound arrays, similar to Xray or Sing-box.

- **`inbounds`**: Defines how local traffic enters the client. Supported types include `tun` (virtual network interface) and `local_proxy` (SOCKS5/HTTP proxy).
- **`outbounds`**: Defines where the client sends the traffic. The main type is `ostp` (encapsulation and transmission to the server), but it also supports `direct` (bypassing the VPN to connect directly to the internet) and `block` (dropping traffic).
- **`routing`**: The mechanism replacing the legacy `exclude` block. It allows for flexible traffic routing based on advanced rules.

Routing rule example in `config.json`:
```json
"routing": {
  "rules": [
    {
      "domain_suffix": ["trusted-site.com", "local.lan"],
      "outbound": "direct"
    }
  ],
  "default_outbound": "proxy"
}
```

> [!NOTE]
> This architecture enables the client to connect to multiple OSTP servers simultaneously, split traffic by domain, or block telemetry directly at the VPN routing level.

---

## Multiplexing

The wire protocol provides support for bundling multiple physical UDP session handles into a single logical transport pipeline via the `"mux"` block:

```json
"mux": {
  "enabled": false,
  "sessions": 1
}
```

### Current Status
Multi-session multiplexing (`sessions > 1`) is supported. Use the `"mux"` block to scale concurrent transport sessions as needed for throughput or resiliency.
