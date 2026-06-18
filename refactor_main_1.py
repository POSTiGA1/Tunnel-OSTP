import os
import re

with open('ostp/src/main.rs', 'r', encoding='utf-8') as f:
    content = f.read()

# 1. Update validation logic in `UnifiedConfig::validate`
content = content.replace('''            if let AppMode::Server(cfg) = &self.mode {
                if cfg.access_keys.is_empty() {
                    anyhow::bail!("Server configuration must contain at least one access_key.");
                }
                if let Some(outbound) = &cfg.outbound {
                    if outbound.enabled {
                        if outbound.protocol != "socks5" {
                            anyhow::bail!("Only SOCKS5 is currently supported for outbound connections.");
                        }
                    }
                }
            }''', '''            if let AppMode::Server(cfg) = &self.mode {
                let mut has_ostp = false;
                for inbound in &cfg.inbounds {
                    if let ostp_server::config::ServerInbound::Ostp { users, .. } = inbound {
                        has_ostp = true;
                        if users.is_empty() {
                            anyhow::bail!("Ostp inbound must contain at least one user.");
                        }
                    }
                }
                if !has_ostp {
                    anyhow::bail!("Server configuration must contain at least one Ostp inbound.");
                }
            }''')

# 2. Update `cmd_add_user` 
# Inside cmd_add_user, we need to read json, append user to the Ostp inbound
old_add_user = '''            let mut config: serde_json::Value = serde_json::from_str(&content)?;
            if let Some(keys) = config.get_mut("access_keys").and_then(|k| k.as_array_mut()) {
                if let Some(meta) = user_meta {
                    let mut obj = serde_json::Map::new();
                    obj.insert("key".to_string(), serde_json::Value::String(key_to_add.clone()));
                    if let Some(name) = meta.name {
                        obj.insert("name".to_string(), serde_json::Value::String(name));
                    }
                    if let Some(limit) = meta.limit_bytes {
                        obj.insert("limit_bytes".to_string(), serde_json::Value::Number(limit.into()));
                    }
                    keys.push(serde_json::Value::Object(obj));
                } else {
                    keys.push(serde_json::Value::String(key_to_add.clone()));
                }
            } else {
                anyhow::bail!("Invalid or missing access_keys array in config.json");
            }
            wizard_save_config(config_path, &config)?;'''

new_add_user = '''            let mut config: serde_json::Value = serde_json::from_str(&content)?;
            let mut added = false;
            if let Some(inbounds) = config.get_mut("inbounds").and_then(|i| i.as_array_mut()) {
                for inbound in inbounds.iter_mut() {
                    if inbound.get("type").and_then(|t| t.as_str()) == Some("ostp") {
                        if let Some(users) = inbound.get_mut("users").and_then(|u| u.as_array_mut()) {
                            if let Some(meta) = &user_meta {
                                let mut obj = serde_json::Map::new();
                                obj.insert("key".to_string(), serde_json::Value::String(key_to_add.clone()));
                                if let Some(name) = &meta.name {
                                    obj.insert("name".to_string(), serde_json::Value::String(name.clone()));
                                }
                                if let Some(limit) = meta.limit_bytes {
                                    obj.insert("limit_bytes".to_string(), serde_json::Value::Number(limit.into()));
                                }
                                users.push(serde_json::Value::Object(obj));
                            } else {
                                users.push(serde_json::Value::String(key_to_add.clone()));
                            }
                            added = true;
                            break;
                        }
                    }
                }
            }
            if !added {
                anyhow::bail!("Could not find Ostp inbound with users array in config.json");
            }
            wizard_save_config(config_path, &config)?;'''
content = content.replace(old_add_user, new_add_user)

# 3. Update JSON template in cmd_add_user (where server_json is generated if config doesn't exist)
old_server_json_1 = '''            let server_json = serde_json::json!({
                "mode": "server",
                "version": "0.3.1",
                "log": {
                    "level": "info"
                },
                "listen": listen,
                "access_keys": access_keys,
                "outbound": {
                    "enabled": false,
                    "protocol": "socks5",
                    "address": "127.0.0.1",
                    "port": 9050,
                    "default_action": "proxy",
                    "rules": []
                },
                "fallback": { "enabled": false, "listen": "0.0.0.0:443", "target": "127.0.0.1:8080" },
                "debug": false
            });'''

new_server_json_1 = '''            let server_json = serde_json::json!({
                "mode": "server",
                "version": "0.3.1",
                "log": {
                    "level": "info"
                },
                "inbounds": [
                    {
                        "type": "ostp",
                        "tag": "ostp-in",
                        "listen": "0.0.0.0",
                        "port": 50000,
                        "users": access_keys
                    }
                ],
                "outbounds": [
                    {
                        "type": "direct",
                        "tag": "direct"
                    }
                ]
            });'''
content = content.replace(old_server_json_1, new_server_json_1)

# 4. Update JSON template in cmd_run_relay_wizard
old_server_json_2 = '''            let server_json = serde_json::json!({
                "mode": "server",
                "version": "0.3.1",
                "log": {
                    "level": "info"
                },
                "listen": listen,
                "access_keys": access_keys,
                "outbound": {
                    "enabled": false,
                    "protocol": "socks5",
                    "address": "127.0.0.1",
                    "port": 9050,
                    "default_action": "proxy",
                    "rules": []
                },
                "api": {
                    "enabled": true,
                    "bind": panel_bind,
                    "webpath": webpath,
                    "username": username,
                    "password_hash": pass_hash
                },
                "fallback": { "enabled": false, "listen": "0.0.0.0:443", "target": "127.0.0.1:8080" },
                "debug": false,
                "license_key": license_key
            });'''

new_server_json_2 = '''            let server_json = serde_json::json!({
                "mode": "server",
                "version": "0.3.1",
                "log": {
                    "level": "info"
                },
                "inbounds": [
                    {
                        "type": "ostp",
                        "tag": "ostp-in",
                        "listen": "0.0.0.0",
                        "port": 50000,
                        "users": access_keys
                    },
                    {
                        "type": "api",
                        "tag": "api-in",
                        "listen": "0.0.0.0",
                        "port": panel_port.parse::<u16>().unwrap_or(9090),
                        "webpath": webpath,
                        "username": username,
                        "password_hash": pass_hash
                    }
                ],
                "outbounds": [
                    {
                        "type": "direct",
                        "tag": "direct"
                    }
                ],
                "license_key": license_key
            });'''
content = content.replace(old_server_json_2, new_server_json_2)

# 5. Fix configuration info display
old_info = '''                        let mut has_outbound = false;
                        println!("{} {} server:", "[ostp]".cyan().bold(), "OSTP".green().bold());
                        println!("  Listen: {:?}", s.listen.primary().as_str().cyan());
                        println!("  Access keys: {}", s.access_keys.len().to_string().yellow());
                        if let Some(api) = &s.api {
                            println!("  Control Panel API: {} (bind: {})", 
                                if api.enabled { "enabled" } else { "disabled" },
                                api.bind.as_str());
                        }
                        if let Some(outbound) = &s.outbound {
                            if outbound.enabled {
                                println!("  Outbound Proxy: SOCKS5 {} (default_action: {})", outbound.address.cyan(), outbound.default_action.as_deref().unwrap_or("proxy").cyan());
                                has_outbound = true;
                            }
                        }
                        if let Some(fb) = &s.fallback {
                            if fb.enabled {
                                println!("  Anti-DPI Fallback: Target {} (bind: {})", fb.target.cyan(), fb.listen.cyan());
                            }
                        }
                        if let Some(dns) = &s.dns {
                            println!("  DNS Proxy: Listen {}", dns.listen.as_deref().unwrap_or("0.0.0.0:53").cyan());
                        }
                        if !has_outbound {
                            println!("  Outbound Proxy: disabled");
                        }'''

new_info = '''                        println!("{} {} server:", "[ostp]".cyan().bold(), "OSTP".green().bold());
                        let mut keys_count = 0;
                        let mut has_outbound = false;
                        for inbound in &s.inbounds {
                            match inbound {
                                ostp_server::config::ServerInbound::Ostp { listen, port, users, fallback, .. } => {
                                    println!("  Inbound OSTP: {}:{}", listen.cyan(), port.to_string().cyan());
                                    keys_count += users.len();
                                    if let Some(fb) = fallback {
                                        if fb.enabled {
                                            println!("    Fallback: -> {}", fb.target.cyan());
                                        }
                                    }
                                }
                                ostp_server::config::ServerInbound::Api { listen, port, .. } => {
                                    println!("  Inbound API: {}:{}", listen.cyan(), port.to_string().cyan());
                                }
                            }
                        }
                        println!("  Access keys: {}", keys_count.to_string().yellow());
                        
                        for ob in &s.outbounds {
                            if let ostp_server::config::ServerOutbound::Socks { server, port, .. } = ob {
                                println!("  Outbound Proxy: SOCKS5 {}:{}", server.cyan(), port.to_string().cyan());
                                has_outbound = true;
                            }
                        }
                        
                        if let Some(dns) = &s.dns {
                            println!("  DNS Proxy: Listen {}", dns.listen.as_deref().unwrap_or("0.0.0.0:53").cyan());
                        }
                        if !has_outbound {
                            println!("  Outbound Proxy: disabled");
                        }'''
content = content.replace(old_info, new_info)


with open('ostp/src/main.rs', 'w', encoding='utf-8') as f:
    f.write(content)
