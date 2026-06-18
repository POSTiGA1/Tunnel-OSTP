import os

with open('ostp/src/main.rs', 'r', encoding='utf-8') as f:
    content = f.read()

old_block = '''                    if let Some(key) = first_key {
                    let host = get_or_ask_public_ip(&args.config);
                    let mut query_params = Vec::<String>::new();
                    query_params.push("type=udp".to_string());

                    let mut link = format!("ostp://{}@{}:{}", key, host, port);
                    if !query_params.is_empty() {
                        link.push('?');
                        link.push_str(&query_params.join("&"));
                    }
                    println!("  [1] {}", link);
                }'''

new_block = '''                    if let Some(key) = first_key {
                        let host = get_or_ask_public_ip(&args.config);
                        let mut query_params = Vec::<String>::new();
                        query_params.push("type=udp".to_string());

                        let mut link = format!("ostp://{}@{}:{}", key, host, port);
                        if !query_params.is_empty() {
                            link.push('?');
                            link.push_str(&query_params.join("&"));
                        }
                        println!("  [1] {}", link);
                    }
                }'''

content = content.replace(old_block, new_block)

with open('ostp/src/main.rs', 'w', encoding='utf-8') as f:
    f.write(content)
