import os

with open('ostp/src/main.rs', 'r', encoding='utf-8') as f:
    content = f.read()

# Fix cmd_list_users
old_list_users = '''fn cmd_list_users(config_path: &std::path::Path, server_cfg: ServerConfig) -> Result<()> {
    println!("{} {} server:", "[ostp]".cyan().bold(), "OSTP".green().bold());
    println!("  Listen: {:?}", server_cfg.listen.primary().as_str().cyan());
    println!("  Access keys: {}", server_cfg.access_keys.len().to_string().yellow());
    
    if server_cfg.access_keys.is_empty() {
        println!("  No users found.");
        return Ok(());
    }

    println!("\n  Users:");
    for (idx, key) in server_cfg.access_keys.iter().enumerate() {
        let name_str = if let Some(n) = key.name() {
            format!(" ({})", n.green())
        } else {
            "".to_string()
        };
        let limit_str = if let Some(l) = key.limit() {
            let l_gb = l as f64 / (1024.0 * 1024.0 * 1024.0);
            format!(" [limit: {:.2} GB]", l_gb.to_string().yellow())
        } else {
            "".to_string()
        };
        println!("  {}. {}{}{}", idx + 1, key.key().cyan(), name_str, limit_str);
    }
    Ok(())
}'''

new_list_users = '''fn cmd_list_users(config_path: &std::path::Path, server_cfg: ServerConfig) -> Result<()> {
    println!("{} {} server:", "[ostp]".cyan().bold(), "OSTP".green().bold());
    
    let mut users = Vec::new();
    for inbound in server_cfg.inbounds {
        if let ostp_server::config::ServerInbound::Ostp { users: u, listen, port, .. } = inbound {
            println!("  Listen: {}:{}", listen.cyan(), port.to_string().cyan());
            users.extend(u);
        }
    }
    
    println!("  Access keys: {}", users.len().to_string().yellow());
    
    if users.is_empty() {
        println!("  No users found.");
        return Ok(());
    }

    println!("\n  Users:");
    for (idx, key) in users.iter().enumerate() {
        let name_str = if let Some(n) = key.name() {
            format!(" ({})", n.green())
        } else {
            "".to_string()
        };
        let limit_str = if let Some(l) = key.limit() {
            let l_gb = l as f64 / (1024.0 * 1024.0 * 1024.0);
            format!(" [limit: {:.2} GB]", l_gb.to_string().yellow())
        } else {
            "".to_string()
        };
        println!("  {}. {}{}{}", idx + 1, key.key().cyan(), name_str, limit_str);
    }
    Ok(())
}'''

content = content.replace(old_list_users, new_list_users)

# Fix commented cmd_list_users in run_app
old_call_list = '''                    // cmd_list_users needs update
                    // cmd_list_users(&args.config, server_cfg)?;'''
new_call_list = '''                    cmd_list_users(&args.config, server_cfg)?;'''
content = content.replace(old_call_list, new_call_list)


with open('ostp/src/main.rs', 'w', encoding='utf-8') as f:
    f.write(content)
