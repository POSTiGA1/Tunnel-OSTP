//! Everything the apps do with a server over SSH, in one place, so the
//! desktop app (Tauri) and the Android app (JNI) share it: add a server,
//! install OSTP on it, read its state through `ostp manage`, change it, and
//! open its web panel through the SSH connection.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::store::{ServerInfo, Store, Vault};
use crate::{shell_quote, Auth, Output, Session, Target};

/// The installed binary (`install.sh` links it here).
const OSTP: &str = "/usr/local/bin/ostp";

/// Release channel to install from. Each has its own branch for the install
/// script, so a beta app installs a beta server that knows the same
/// `ostp manage` commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Stable,
    Beta,
    Alpha,
}

impl Channel {
    /// From the app's own version: `0.4.6-beta.7` is beta.
    pub fn of_version(version: &str) -> Channel {
        if version.contains("-alpha") {
            Channel::Alpha
        } else if version.contains("-beta") {
            Channel::Beta
        } else {
            Channel::Stable
        }
    }

    fn branch(self) -> (&'static str, &'static str) {
        // (git branch of install.sh, release channel it installs)
        match self {
            Channel::Stable => ("master", "stable"),
            Channel::Beta => ("beta", "beta"),
            Channel::Alpha => ("alpha", "alpha"),
        }
    }
}

/// Changes that print plain text rather than JSON.
#[derive(Clone, Debug)]
pub enum Action {
    /// Update OSTP to the newest release of the channel
    Update(Channel),
    Restart,
    Reboot,
    /// Remove OSTP from the server, config included
    Uninstall,
    /// Turn the web panel on with this sign-in
    PanelEnable { user: String, password: String },
    PanelDisable,
    /// Domain and Let's Encrypt certificate
    CertIssue { domain: String, email: Option<String> },
    SubEnable,
    SubDisable,
}

impl Action {
    /// The command and what it reads on stdin.
    fn command(&self) -> (String, Option<String>) {
        match self {
            Action::Update(ch) => (format!("{OSTP} update -b {}", ch.branch().1), None),
            Action::Restart => ("systemctl restart ostp && sleep 2 && systemctl is-active ostp".into(), None),
            // Detached, so the command returns before the connection drops.
            Action::Reboot => ("nohup sh -c 'sleep 2; reboot' >/dev/null 2>&1 &".into(), None),
            Action::Uninstall => (format!("{OSTP} uninstall"), None),
            Action::PanelEnable { user, password } => (
                format!("{OSTP} panel enable --user {}", shell_quote(user)),
                Some(format!("{password}\n")),
            ),
            Action::PanelDisable => (format!("{OSTP} panel disable"), None),
            Action::CertIssue { domain, email } => {
                let mut cmd = format!("{OSTP} cert issue --yes --agree-tos --domain {}", shell_quote(domain));
                if let Some(e) = email.as_deref().filter(|e| !e.is_empty()) {
                    cmd.push_str(&format!(" --email {}", shell_quote(e)));
                }
                (cmd, None)
            }
            Action::SubEnable => (format!("{OSTP} sub enable"), None),
            Action::SubDisable => (format!("{OSTP} sub disable"), None),
        }
    }
}

pub struct Manager {
    store: Mutex<Store>,
    vault: Vault,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    /// Local ports forwarded to each server's panel.
    panels: Mutex<HashMap<String, u16>>,
}

impl Manager {
    /// `servers_file`: where the list is kept. `master_key`: from the OS
    /// credential store, `None` when there is none.
    pub fn new(servers_file: &Path, master_key: Option<[u8; 32]>) -> Result<Manager> {
        Ok(Manager {
            store: Mutex::new(Store::load(servers_file)?),
            vault: Vault::new(master_key),
            sessions: Mutex::new(HashMap::new()),
            panels: Mutex::new(HashMap::new()),
        })
    }

    pub fn can_remember(&self) -> bool {
        self.vault.can_remember()
    }

    pub async fn list(&self) -> Vec<ServerInfo> {
        self.store.lock().await.list()
    }

    /// Connects (checking the password or key) and saves the server.
    pub async fn add(&self, name: &str, target: Target, auth: Auth, remember: bool) -> Result<ServerInfo> {
        let session = Session::connect(&target, &auth, None).await?;
        let info = self.store.lock().await.add(&self.vault, name, &target, &auth, session.host_key(), remember)?;
        self.sessions.lock().await.insert(info.id.clone(), Arc::new(session));
        Ok(info)
    }

    pub async fn rename(&self, id: &str, name: &str) -> Result<()> {
        self.store.lock().await.rename(id, name)
    }

    /// Forgets the server (nothing on the server changes).
    pub async fn remove(&self, id: &str) -> Result<()> {
        if let Some(s) = self.sessions.lock().await.remove(id) {
            s.close().await;
        }
        self.panels.lock().await.remove(id);
        self.store.lock().await.remove(id)
    }

    /// The open connection to a server, connecting when there is none.
    /// `auth` is what the app asked for when the secret is not remembered;
    /// without it such a server fails with `SECRET_NEEDED`.
    pub async fn session(&self, id: &str, auth: Option<Auth>) -> Result<Arc<Session>> {
        if let Some(s) = self.sessions.lock().await.get(id) {
            if s.run("true").await.is_ok_and(|o| o.success()) {
                return Ok(s.clone());
            }
        }
        let (target, host_key, auth) = {
            let store = self.store.lock().await;
            let server = store.get(id)?;
            (server.target(), server.host_key.clone(), store.auth(&self.vault, id, auth)?)
        };
        let session = Arc::new(Session::connect(&target, &auth, Some(&host_key)).await?);
        self.sessions.lock().await.insert(id.to_string(), session.clone());
        self.panels.lock().await.remove(id);
        Ok(session)
    }

    /// Whether OSTP is installed, and which version.
    pub async fn probe(&self, id: &str, auth: Option<Auth>) -> Result<Value> {
        let s = self.session(id, auth).await?;
        let out = s
            .run_root(&format!(
                "uname -m; (. /etc/os-release 2>/dev/null && echo \"$PRETTY_NAME\") || uname -s; \
                 command -v systemctl >/dev/null && echo systemd || echo no-systemd; \
                 {OSTP} --version 2>/dev/null || echo none"
            ))
            .await?;
        if !out.success() {
            bail!("{}", first_line(&out));
        }
        let lines: Vec<&str> = out.stdout.lines().collect();
        let version = lines.get(3).copied().unwrap_or("none");
        Ok(serde_json::json!({
            "arch": lines.first(),
            "os": lines.get(1),
            "systemd": lines.get(2) == Some(&"systemd"),
            "installed": version != "none",
            "version": version.rsplit(' ').next(),
        }))
    }

    /// Installs OSTP (or updates it, when it is there; the config is kept)
    /// and returns the users and their links. A first install writes the
    /// config and starts the service through `install.sh -y`.
    /// `on_line` receives the installer's output as it runs.
    pub async fn install(&self, id: &str, auth: Option<Auth>, channel: Channel, port: u16, on_line: impl FnMut(&str)) -> Result<Value> {
        let s = self.session(id, auth).await?;
        let host = self.store.lock().await.get(id)?.host.clone();
        let (branch, release) = channel.branch();
        let script = format!("https://raw.githubusercontent.com/ospab/ostp/{branch}/scripts/install.sh");
        // curl is missing on some minimal images; install it the way the
        // system does. The config is left alone when one is already there.
        let cmd = format!(
            "set -e; export DEBIAN_FRONTEND=noninteractive; \
             if ! command -v curl >/dev/null; then \
               (apt-get update -q && apt-get install -yq curl) || dnf install -y curl || yum install -y curl; fi; \
             curl -fsSL {script} -o /tmp/ostp-install.sh; \
             bash /tmp/ostp-install.sh -y -b {release} --port {port} --host {host}; \
             rm -f /tmp/ostp-install.sh; \
             {OSTP} manage users",
            script = shell_quote(&script),
            host = shell_quote(&host),
        );
        let out = s.run_root_with(&cmd, on_line).await?;
        parse_manage(&out)
    }

    /// `ostp manage <args>`: status, users, user-add NAME, ...
    pub async fn manage(&self, id: &str, auth: Option<Auth>, args: &[&str]) -> Result<Value> {
        let s = self.session(id, auth).await?;
        let cmd = std::iter::once(format!("{OSTP} manage"))
            .chain(args.iter().map(|a| shell_quote(a)))
            .collect::<Vec<_>>()
            .join(" ");
        parse_manage(&s.run_root(&cmd).await?)
    }

    /// Runs a change and returns its output; fails when it fails.
    pub async fn action(&self, id: &str, auth: Option<Auth>, action: Action, on_line: impl FnMut(&str)) -> Result<Output> {
        let s = self.session(id, auth).await?;
        let (cmd, stdin) = action.command();
        let cmd = match stdin {
            // Through the shell, so the password never appears on a command line.
            Some(input) => format!("printf '%s' {} | {cmd}", shell_quote(&input)),
            None => cmd,
        };
        let out = s.run_root_with(&cmd, on_line).await?;
        if matches!(action, Action::Reboot) {
            self.sessions.lock().await.remove(id);
        }
        if !out.success() {
            bail!("{}", first_line(&out));
        }
        Ok(out)
    }

    /// Opens the server's web panel through this SSH connection and returns
    /// the local address to open in a browser. The panel must be on.
    pub async fn open_panel(&self, id: &str, auth: Option<Auth>) -> Result<String> {
        let status = self.manage(id, auth.clone(), &["status"]).await?;
        let panel = &status["panel"];
        if !panel["enabled"].as_bool().unwrap_or(false) {
            bail!("PANEL_OFF");
        }
        let bind = panel["bind"].as_str().unwrap_or("127.0.0.1:9090");
        let remote_port: u16 = bind.rsplit(':').next().and_then(|p| p.parse().ok()).context("the panel has no port")?;
        let webpath = panel["webpath"].as_str().unwrap_or("").trim_matches('/').to_string();
        let local = match self.panels.lock().await.get(id).copied() {
            Some(p) => p,
            None => {
                let s = self.session(id, auth).await?;
                let p = s.forward_local(remote_port).await?;
                self.panels.lock().await.insert(id.to_string(), p);
                p
            }
        };
        Ok(if webpath.is_empty() { format!("http://127.0.0.1:{local}/") } else { format!("http://127.0.0.1:{local}/{webpath}/") })
    }
}

fn first_line(out: &Output) -> String {
    let text = if out.stderr.trim().is_empty() { &out.stdout } else { &out.stderr };
    text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("the command failed").trim().to_string()
}

/// The JSON `ostp manage` printed, or its error.
fn parse_manage(out: &Output) -> Result<Value> {
    let line = out.last_json_line().ok_or_else(|| {
        if out.stdout.contains("unrecognized subcommand 'manage'") || out.stderr.contains("unrecognized subcommand 'manage'") {
            anyhow!("the OSTP on the server is too old for the app; update it")
        } else {
            anyhow!("{}", first_line(out))
        }
    })?;
    let v: Value = serde_json::from_str(line).context("the server's answer is not valid JSON")?;
    if let Some(e) = v.get("error").and_then(|e| e.as_str()) {
        bail!("{e}");
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_from_the_apps_version() {
        assert_eq!(Channel::of_version("0.4.6-beta.7"), Channel::Beta);
        assert_eq!(Channel::of_version("v0.4.6-alpha.2"), Channel::Alpha);
        assert_eq!(Channel::of_version("0.4.6"), Channel::Stable);
    }

    #[test]
    fn the_panel_password_is_passed_on_stdin() {
        let (cmd, stdin) = Action::PanelEnable { user: "ad'min".into(), password: "p w".into() }.command();
        assert_eq!(cmd, "/usr/local/bin/ostp panel enable --user 'ad'\\''min'");
        assert_eq!(stdin.as_deref(), Some("p w\n"));
    }

    #[test]
    fn manage_answers_and_errors() {
        let ok = Output { stdout: "noise\n{\"users\":[]}".into(), ..Default::default() };
        assert_eq!(parse_manage(&ok).unwrap()["users"], serde_json::json!([]));
        let err = Output { status: 1, stdout: "{\"error\":\"no user matches x\"}".into(), ..Default::default() };
        assert_eq!(parse_manage(&err).unwrap_err().to_string(), "no user matches x");
        let old = Output { status: 2, stderr: "error: unrecognized subcommand 'manage'".into(), ..Default::default() };
        assert!(parse_manage(&old).unwrap_err().to_string().contains("too old"));
    }
}
