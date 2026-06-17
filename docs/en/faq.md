# Frequently Asked Questions (FAQ)

## What is OSTP and how does it differ from other VPNs (WireGuard, OpenVPN)?
OSTP is a protocol built from the ground up for maximum Deep Packet Inspection (DPI) evasion. Unlike WireGuard and OpenVPN, which have recognizable handshakes and static headers, OSTP obfuscates 100% of the data starting from the very first byte. Every packet is indistinguishable from random white noise, making static filtering impossible.

## How does DPI evasion work? Is it secure?
OSTP architecture strictly adheres to **Kerckhoffs's Principle**. The code is fully open source and does not rely on security by obscurity. The obfuscation is backed by rigorous cryptographic algorithms (Noise Protocol, ChaCha20Poly1305, Blake2s) and pre-shared keys. Censors and DPI systems cannot write a signature or filter for OSTP because there are simply no repetitive patterns in the traffic.

## How do I upgrade to version 0.3.1 and what happens to `config.json`?
Version 0.3.1 introduced a new modular architecture (`inbounds` and `outbounds` arrays). When you run OSTP v0.3.1+ with an older configuration file, the built-in auto-migrator automatically converts it to the new format without data loss and appends `"version": "0.3.1"`.

## Why is multiplexing not working for me (sessions > 1)?
There is a known issue within the `mux` demultiplexer when handling multiple sessions concurrently. The handshake succeeds, but application data fails to stream. Please keep the session count to 1 or disable `mux` entirely until a patch is released in future `ostp-core` versions.

## Is there proprietary or closed-source code in OSTP?
The core protocol engine and base client/server implementations are completely open source and available for peer review in this repository. However, certain experimental or enterprise-specific tooling (`ostp-brain`, `ostp-prober`, `ostp-sandbox`, and parts of `ostp-gui`) are excluded from the public workspace to keep the open-source codebase focused.
