# Native Integrations

## Cross-Platform Engineering
The OSTP core protocol (`ostp-core`) is `sans-io` and completely platform-agnostic. Each host application links it and supplies the actual networking, TUN adapter, and UI — described below.

## Mobile: Flutter App + Android JNI Bindings
`ostp-flutter` is the cross-platform mobile app (Flutter, Android today). It embeds the Rust client engine on Android through `ostp-jni`, a dedicated crate exporting a small C-ABI/JNI surface (`Java_net_ostp_client_OstpClientSdk_*`: start/stop the client, fetch metrics, fetch logs, push a log line, notify of a network change) consumed by the app's Kotlin layer (`OstpClientSdk.kt`, `OstpVpnService.kt`).

- **Isolated runtime**: `ostp-jni` spins up and owns its own multithreaded Tokio runtime inside the host process, so tunnel I/O never blocks the Android UI thread.
- **Telemetry bridge**: metrics and logs are polled across the JNI boundary through plain, memory-safe getter calls rather than a push channel, keeping the FFI surface small.

## Desktop GUI
`ostp-gui` is a Tauri v2 desktop application (Windows-focused today) that drives the same client engine through its own bridge layer, offering system-proxy and TUN modes, share-link import, and exclusion management. See the [GUI Client wiki page](https://github.com/ospab/ostp/wiki/GUI-Client).

### Privilege separation for TUN
Creating a TUN adapter requires elevated privileges; running the whole GUI elevated is undesirable. Instead, TUN setup is delegated to `ostp-tun-helper`, a small standalone process the GUI launches and talks to over a local command channel (start/stop with a config + auth token) — only that helper, not the GUI, needs to run elevated. The actual adapter creation code (Windows/Wintun, Linux, macOS) lives in the shared `ostp-tun` crate, used by both the helper and the CLI's own native TUN path (see [`client.md`](client.md)).

## System Interfaces
On desktop, platform-specific modules handle host integration:
- **Windows system proxy** (`sysproxy.rs`): writes/restores WinINet proxy registry values for zero-configuration browser proxying.
- **TUN adapters** (`ostp-tun`): Wintun on Windows, native TUN devices on Linux/macOS; a userspace `netstack-smoltcp` stack reconstructs TCP/UDP flows from the raw IP packets the adapter delivers.
- **Process-based exclusion** (Windows): matches a local TCP connection back to its owning process via `GetExtendedTcpTable`, so per-process bypass rules (§[`client.md`](client.md)) can be enforced without a kernel driver.
