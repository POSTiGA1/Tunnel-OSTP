# OSTP Configuration Migration to v0.3.1

The OSTP `config.json` schema has been significantly redesigned in version `v0.3.1` to support a modern multi-server architecture. The new schema provides greater flexibility by splitting configuration into `inbounds`, `outbounds`, and flexible `routing` rules, replacing the monolithic architecture of previous versions.

## Automatic Migration

The OSTP core and GUI clients are equipped with an automatic migrator. When launching OSTP `v0.3.1` with a `config.json` from a previous version, the migrator will automatically transform the legacy schema into the new `v0.3.1` schema.

The migrated file will be overwritten with the new format and will begin with:
```json
// OSTP Configuration v0.3.1
// DO NOT EDIT THIS COMMENT - Migrator relies on it
{
  "version": "0.3.1",
  "mode": "client",
  ...
}
```

## Manual Schema Reference

If you prefer to configure manually, the following is a reference of the new modular configuration format:

### Legacy Configuration (v0.2.x)
```json
{
  "mode": "client",
  "server": "192.168.1.100:50000",
  "access_key": "mysecretkey",
  "socks5_bind": "127.0.0.1:1088",
  "tun": {
    "enable": true,
    "kill_switch": true
  },
  "exclude": {
    "domains": ["localhost"],
    "ips": ["192.168.1.0/24"]
  }
}
```

### New Configuration (v0.3.1)
```json
{
  "version": "0.3.1",
  "mode": "client",
  "api": {
    "enabled": true,
    "bind": "127.0.0.1:50001",
    "token": "admin-secret-token"
  },
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
      "type": "socks",
      "tag": "socks-in",
      "bind_addr": "127.0.0.1:1088"
    }
  ],
  "outbounds": [
    {
      "type": "ostp",
      "tag": "proxy",
      "server": "192.168.1.100",
      "port": 50000,
      "access_key": "mysecretkey",
      "transport": {
        "type": "udp"
      },
      "multiplex": {
        "enabled": false
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
        "ip_cidr": ["192.168.1.0/24"],
        "outbound": "direct"
      }
    ],
    "default_outbound": "proxy"
  }
}
```

### Key Changes
- **Outbounds List**: Multiple proxy servers can now be defined.
- **Inbounds List**: TUN and SOCKS5 are now independent listeners.
- **Routing**: Fine-grained traffic routing between inbounds and outbounds based on domains, IPs, and processes.
- **Comments**: The GUI and migrator now use JS-style `//` comments in `config.json` instead of the legacy `"_comment"` JSON keys.
