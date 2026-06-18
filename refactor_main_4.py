import os

with open('ostp/src/main.rs', 'r', encoding='utf-8') as f:
    content = f.read()

old_run_app_block = '''        AppMode::Server(server_cfg) => {
            println!("{}", include_str!("../../docs/banner.txt").blue().bold());
            
            let listen_addrs = server_cfg.listen.addresses();
            println!("{} Starting server on {:?}", "[ostp]".cyan().bold(), listen_addrs);
            let debug = server_cfg.debug.unwrap_or(false);
            let outbound = server_cfg.outbound.map(|o| ostp_server::OutboundConfig {
                enabled: o.enabled,
                protocol: o.protocol,
                address: o.address,
                port: o.port,
                rules: o
                    .rules
                    .into_iter()
                    .map(|r| ostp_server::OutboundRule {
                        domain_suffix: r.domain_suffix.unwrap_or_default(),
                        ip_cidr: r.ip_cidr.unwrap_or_default(),
                        protocol: r.protocol,
                        action: parse_outbound_action(r.action),
                    })
                    .collect(),
                default_action: parse_outbound_action(o.default_action),
            });
            let api_config = server_cfg.api.map(|a| ostp_server::ApiConfig {
                enabled: a.enabled.unwrap_or(false),
                bind: a.bind.unwrap_or_else(|| "127.0.0.1:9090".to_string()),
                token: a.token.clone(),
                webpath: a.webpath.unwrap_or_default(),
                username: a.username.unwrap_or_default(),
                password_hash: a.password_hash.unwrap_or_default(),
            });
            let fallback_config = server_cfg.fallback.map(|f| ostp_server::FallbackConfig {
                enabled: f.enabled.unwrap_or(false),
                listen: f.listen.unwrap_or_else(|| "0.0.0.0:443".to_string()),
                target: f.target.unwrap_or_else(|| "127.0.0.1:8080".to_string()),
            });

            let access_keys_meta = server_cfg.access_keys.into_iter().map(|uc| {
                (uc.key(), ostp_server::api::UserMeta {
                    name: uc.name(),
                    limit_bytes: uc.limit(),
                })
            }).collect::<Vec<_>>();
            let host = get_or_ask_public_ip(&args.config);
            // Build DNS config and set owndns flag in subscribe links if DNS enabled
            let dns_cfg = server_cfg.dns;
            // Pass all listen addresses for multi-listener support
            ostp_server::run_server(listen_addrs, Some(host), access_keys_meta, outbound, api_config, fallback_config, debug, dns_cfg, Some(args.config), server_cfg.license_key.clone()).await?;
        }'''

new_run_app_block = '''        AppMode::Server(server_cfg) => {
            println!("{}", include_str!("../../docs/banner.txt").blue().bold());
            
            let mut listen_addrs = Vec::new();
            let mut access_keys_meta = Vec::new();
            let mut fallback_config = None;
            let mut host_port = ("0.0.0.0".to_string(), 50000);
            let mut api_config = None;

            for inbound in server_cfg.inbounds {
                match inbound {
                    ostp_server::config::ServerInbound::Ostp { listen, port, users, fallback, .. } => {
                        listen_addrs.push(format!("{}:{}", listen, port));
                        host_port = (listen.clone(), port);
                        for uc in users {
                            access_keys_meta.push((uc.key(), ostp_server::api::UserMeta {
                                name: uc.name(),
                                limit_bytes: uc.limit(),
                            }));
                        }
                        if fallback_config.is_none() {
                            fallback_config = fallback;
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

            println!("{} Starting server on {:?}", "[ostp]".cyan().bold(), listen_addrs);
            let debug = server_cfg.debug.unwrap_or(false);
            
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
                    break;
                }
            }

            let dns_cfg = server_cfg.dns;
            
            let host = if host_port.0 == "0.0.0.0" {
                detect_local_public_ip().unwrap_or_else(|| "127.0.0.1".to_string())
            } else {
                host_port.0.to_string()
            };

            ostp_server::run_server(listen_addrs, Some(host), access_keys_meta, outbound, api_config, fallback_config, debug, dns_cfg, Some(args.config), server_cfg.license_key.clone()).await?;
        }'''

content = content.replace(old_run_app_block, new_run_app_block)

with open('ostp/src/main.rs', 'w', encoding='utf-8') as f:
    f.write(content)
