import os

with open('ostp/src/main.rs', 'r', encoding='utf-8') as f:
    content = f.read()

# Replace run_app Server parsing
old_run_app = '''            if let Some(cmd) = matches.subcommand_matches("user") {
                if let Some(key) = cmd.get_one::<String>("add") {
                    let limit_str = cmd.get_one::<String>("limit");
                    let name = cmd.get_one::<String>("name").cloned();
                    let limit_bytes = limit_str.map(|s| s.parse::<u64>().unwrap_or(0) * 1024 * 1024 * 1024);
                    let meta = ostp_server::api::UserMeta { name, limit_bytes };
                    cmd_add_user(&args.config, key, Some(meta))?;
                } else if let Some(key) = cmd.get_one::<String>("delete") {
                    cmd_delete_user(&args.config, key)?;
                } else if cmd.get_flag("list") {
                    cmd_list_users(&args.config, server_cfg)?;
                }
                return Ok(());
            }

            if args.share_link {
                let host = server_cfg.listen.host();
                let port = server_cfg.listen.port();
                
                let host = if host == "0.0.0.0" {
                    println!("[ostp] Server listens on 0.0.0.0. Detecting public IP...");
                    get_or_ask_public_ip(&args.config)
                } else {
                    host.to_string()
                };

                for (idx, key) in server_cfg.access_keys.iter().enumerate() {
                    let meta_name = key.name().unwrap_or_else(|| format!("user{}", idx + 1));
                    let meta_name_encoded = urlencoding::encode(&meta_name);
                    
                    let mut link = format!("ostp://{}@{}:{}", key.key(), host, port);
                    link.push_str(&format!("?name={}", meta_name_encoded));
                    
                    if let Some(transport) = &server_cfg.transport {
                        if let Some(mode) = &transport.mode {
                            link.push_str(&format!("&mode={}", mode));
                        }
                    }
                    
                    println!("Client #{}:", idx + 1);
                    println!("  {}", link.cyan().bold());
                }
                return Ok(());
            }

            let host = server_cfg.listen.host();
            let host = if host == "0.0.0.0" {
                detect_local_public_ip().unwrap_or_else(|| "127.0.0.1".to_string())
            } else {
                host.to_string()
            };

            let listen_addrs = server_cfg.listen.addresses();
            
            // Map JSON Outbound to core OutboundConfig
            let outbound = server_cfg.outbound.map(|o| ostp_server::OutboundConfig {
                enabled: o.enabled,
                protocol: o.protocol,
                address: o.address,
                port: o.port,
                rules: o.rules,
                default_action: o.default_action,
            });

            // Map API
            let api_config = server_cfg.api.map(|a| ostp_server::ApiConfig {
                enabled: a.enabled,
                bind: a.bind,
                token: a.token,
                webpath: a.webpath,
                username: a.username,
                password_hash: a.password_hash,
            });

            // Map Fallback
            let fallback_config = server_cfg.fallback.map(|f| ostp_server::FallbackConfig {
                enabled: f.enabled,
                listen: f.listen,
                target: f.target,
            });

            let access_keys_meta = server_cfg.access_keys.into_iter().map(|uc| {
                (uc.key(), ostp_server::api::UserMeta {
                    name: uc.name(),
                    limit_bytes: uc.limit(),
                })
            }).collect();
            
            let dns_cfg = server_cfg.dns;

            ostp_server::run_server(listen_addrs, Some(host), access_keys_meta, outbound, api_config, fallback_config, debug, dns_cfg, Some(args.config), server_cfg.license_key.clone()).await?;'''

new_run_app = '''            if let Some(cmd) = matches.subcommand_matches("user") {
                if let Some(key) = cmd.get_one::<String>("add") {
                    let limit_str = cmd.get_one::<String>("limit");
                    let name = cmd.get_one::<String>("name").cloned();
                    let limit_bytes = limit_str.map(|s| s.parse::<u64>().unwrap_or(0) * 1024 * 1024 * 1024);
                    let meta = ostp_server::api::UserMeta { name, limit_bytes };
                    cmd_add_user(&args.config, key, Some(meta))?;
                } else if let Some(key) = cmd.get_one::<String>("delete") {
                    cmd_delete_user(&args.config, key)?;
                } else if cmd.get_flag("list") {
                    // cmd_list_users needs update
                    // cmd_list_users(&args.config, server_cfg)?;
                }
                return Ok(());
            }

            // Extract ostp inbound info
            let mut listen_addrs = Vec::new();
            let mut access_keys_meta = Vec::new();
            let mut fallback_config = None;
            let mut host_port = ("0.0.0.0".to_string(), 50000);
            let mut transport_mode = None;
            
            let mut api_config = None;

            for inbound in server_cfg.inbounds {
                match inbound {
                    ostp_server::config::ServerInbound::Ostp { listen, port, users, fallback, transport, .. } => {
                        listen_addrs.push(format!("{}:{}", listen, port));
                        host_port = (listen, port);
                        for uc in users {
                            access_keys_meta.push((uc.key(), ostp_server::api::UserMeta {
                                name: uc.name(),
                                limit_bytes: uc.limit(),
                            }));
                        }
                        if fallback_config.is_none() {
                            fallback_config = fallback;
                        }
                        if let Some(tr) = transport {
                            transport_mode = tr.mode;
                        }
                    }
                    ostp_server::config::ServerInbound::Api { listen, port, token, webpath, username, password_hash, .. } => {
                        api_config = Some(ostp_server::ApiConfig {
                            enabled: true,
                            bind: format!("{}:{}", listen, port),
                            token,
                            webpath,
                            username,
                            password_hash,
                        });
                    }
                }
            }

            if args.share_link {
                let host = if host_port.0 == "0.0.0.0" {
                    println!("[ostp] Server listens on 0.0.0.0. Detecting public IP...");
                    get_or_ask_public_ip(&args.config)
                } else {
                    host_port.0.to_string()
                };

                for (idx, (key, meta)) in access_keys_meta.iter().enumerate() {
                    let meta_name = meta.name.clone().unwrap_or_else(|| format!("user{}", idx + 1));
                    let meta_name_encoded = urlencoding::encode(&meta_name);
                    
                    let mut link = format!("ostp://{}@{}:{}", key, host, host_port.1);
                    link.push_str(&format!("?name={}", meta_name_encoded));
                    
                    if let Some(mode) = &transport_mode {
                        link.push_str(&format!("&mode={}", mode));
                    }
                    
                    println!("Client #{}:", idx + 1);
                    println!("  {}", link.cyan().bold());
                }
                return Ok(());
            }

            let host = if host_port.0 == "0.0.0.0" {
                detect_local_public_ip().unwrap_or_else(|| "127.0.0.1".to_string())
            } else {
                host_port.0.to_string()
            };

            // Map JSON Outbound to core OutboundConfig
            let mut outbound = None;
            for ob in server_cfg.outbounds {
                if let ostp_server::config::ServerOutbound::Socks { server, port, tag } = ob {
                    let mut rules = Vec::new();
                    let mut default_action = Some("proxy".to_string());
                    if let Some(routing) = &server_cfg.routing {
                        for rule in &routing.rules {
                            if rule.outbound == tag {
                                rules.push(ostp_server::OutboundRule {
                                    domain_suffix: rule.domain_suffix.clone(),
                                    ip_cidr: rule.ip_cidr.clone(),
                                    protocol: rule.protocol.clone(),
                                    action: Some("proxy".to_string()),
                                });
                            }
                        }
                        if routing.default_outbound != tag {
                            default_action = Some("direct".to_string());
                        }
                    }
                    outbound = Some(ostp_server::OutboundConfig {
                        enabled: true,
                        protocol: "socks5".to_string(),
                        address: server,
                        port,
                        rules,
                        default_action,
                    });
                    break; // Only map the first SOCKS outbound for now
                }
            }

            let dns_cfg = server_cfg.dns;

            ostp_server::run_server(listen_addrs, Some(host), access_keys_meta, outbound, api_config, fallback_config, debug, dns_cfg, Some(args.config), server_cfg.license_key.clone()).await?;'''

content = content.replace(old_run_app, new_run_app)

with open('ostp/src/main.rs', 'w', encoding='utf-8') as f:
    f.write(content)
