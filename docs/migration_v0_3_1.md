# OSTP v0.3.1 Configuration Migration

In OSTP version 0.3.1, we have completely overhauled the `config.json` architecture for the client. The old monolithic structure (where all settings were in the root object) has been replaced by a modular system based on arrays of `inbounds` (incoming connections) and `outbounds` (outgoing connections), similar to Xray/V2Ray/Sing-box.

This allows OSTP to scale, support multiple proxy servers, multiple entry points (SOCKS5, TUN), and complex routing (`routing`).

## Automatic Migration

The `ostp` core includes a built-in automatic migrator. Upon starting any program (cli, gui, flutter), the core will check your `config.json`. 

If the configuration lacks the `"version": "0.3.1"` field, OSTP will **automatically** convert your old config into the new modular format and save it to disk without data loss.

### What happens during migration:

1. **TUN and SOCKS5** -> converted into the `inbounds` array.
   - The `socks5_bind` setting becomes an inbound `local_proxy` (SOCKS).
   - The `tun` setting becomes an inbound `tun`.
2. **OSTP Server** -> moved into the `outbounds` array.
   - Parameters `server`, `access_key`, `transport`, `mux` are combined into an outbound of type `"ostp"`.
3. **Split Tunneling (Exclude)** -> converted into `routing` rules.
   - Old `domains` and `ips` are converted into rules routing traffic to the `"direct"` outbound.
   - All other requests are routed by default to the `"proxy"` outbound.
4. **`version` fields**
   - The field `"version": "0.3.1"` is added to prevent re-migration in the future. The `_comment` field has been removed.

## Change Example

### Before 0.3.1 (Old format)
```json
{
  "mode": "client",
  "log_level": "info",
  "server": "1.2.3.4:50000",
  "access_key": "secret",
  "socks5_bind": "127.0.0.1:1088",
  "tun": {
    "enable": true
  },
  "exclude": {
    "domains": ["localhost"]
  }
}
```

### After 0.3.1 (New format)
```json
{
  "mode": "client",
  "version": "0.3.1",
  "log": {
    "level": "info"
  },
  "inbounds": [
    {
      "type": "tun",
      "tag": "tun-in",
      "auto_route": true,
      "mtu": 1140
    },
    {
      "type": "local_proxy",
      "tag": "socks-in",
      "protocol": "socks",
      "listen": "127.0.0.1",
      "port": 1088
    }
  ],
  "outbounds": [
    {
      "type": "ostp",
      "tag": "proxy",
      "server": "1.2.3.4",
      "port": 50000,
      "access_key": "secret",
      "transport": {
        "type": "udp"
      }
    },
    {
      "type": "direct",
      "tag": "direct"
    },
    {
      "type": "block",
      "tag": "block"
    }
  ],
  "routing": {
    "rules": [
      {
        "domain_suffix": ["localhost"],
        "outbound": "direct"
      }
    ],
    "default_outbound": "proxy"
  }
}
```

## Information for GUI Developers (ostp-gui, ostp-flutter)

If you are developing integrations or third-party clients, **you no longer need to parse the old fields**. You should use the `inbounds` and `outbounds` arrays. If the GUI passes a `serde_json::Value` to the core, the core will migrate it itself before starting. However, to save changes from the UI, you must modify the new array structure explicitly.
