//! Against a real OpenSSH server started for the test. Needs root (to start
//! sshd and create a test user) and `sshd`, `ssh-keygen`, `sudo`, so it is
//! ignored by default:
//!
//! ```sh
//! sudo cargo test -p ostp-ssh --test sshd -- --ignored --test-threads=1
//! ```

use ostp_ssh::{Auth, HostKeyChanged, Session, Target};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const USER: &str = "ostpsshtest";
const PASSWORD: &str = "Test-pass-123";

struct Sshd {
    child: Child,
    port: u16,
    dir: PathBuf,
}

impl Drop for Sshd {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn run(cmd: &str, args: &[&str]) {
    let st = Command::new(cmd).args(args).status().unwrap();
    assert!(st.success(), "{cmd} {args:?} failed");
}

fn keygen(path: &Path, kind: &str) {
    let _ = std::fs::remove_file(path);
    run("ssh-keygen", &["-q", "-t", kind, "-N", "", "-f", path.to_str().unwrap()]);
}

fn start_sshd() -> Sshd {
    let dir = std::env::temp_dir().join(format!("ostp-sshd-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    keygen(&dir.join("host_ed25519"), "ed25519");
    keygen(&dir.join("client_ed25519"), "ed25519");
    std::fs::copy(dir.join("client_ed25519.pub"), dir.join("authorized_keys")).unwrap();

    // The password user with sudo rights.
    if Command::new("id").arg(USER).output().is_ok_and(|o| !o.status.success()) {
        run("useradd", &["-m", "-s", "/bin/sh", USER]);
    }
    let mut chpasswd = Command::new("chpasswd").stdin(std::process::Stdio::piped()).spawn().unwrap();
    chpasswd.stdin.take().unwrap().write_all_sync(format!("{USER}:{PASSWORD}\n").as_bytes());
    assert!(chpasswd.wait().unwrap().success());
    std::fs::write(format!("/etc/sudoers.d/{USER}"), format!("{USER} ALL=(ALL) ALL\n")).unwrap();
    std::fs::create_dir_all("/run/sshd").unwrap();

    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let config = dir.join("sshd_config");
    std::fs::write(
        &config,
        format!(
            "Port {port}\nListenAddress 127.0.0.1\nHostKey {}\nAuthorizedKeysFile {}\nStrictModes no\n\
             PasswordAuthentication yes\nKbdInteractiveAuthentication no\nPermitRootLogin prohibit-password\n\
             UsePAM yes\nAllowTcpForwarding yes\nPidFile {}\n",
            dir.join("host_ed25519").display(),
            dir.join("authorized_keys").display(),
            dir.join("sshd.pid").display(),
        ),
    )
    .unwrap();
    let child = Command::new("/usr/sbin/sshd").args(["-D", "-e", "-f", config.to_str().unwrap()]).spawn().unwrap();
    for _ in 0..50 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Sshd { child, port, dir }
}

trait WriteAllSync {
    fn write_all_sync(self, data: &[u8]);
}
impl WriteAllSync for std::process::ChildStdin {
    fn write_all_sync(mut self, data: &[u8]) {
        std::io::Write::write_all(&mut self, data).unwrap();
    }
}

fn host_fingerprint(dir: &Path) -> String {
    let out = Command::new("ssh-keygen").args(["-l", "-E", "sha256", "-f"]).arg(dir.join("host_ed25519.pub")).output().unwrap();
    String::from_utf8(out.stdout).unwrap().split_whitespace().nth(1).unwrap().to_string()
}

fn target(sshd: &Sshd, user: &str) -> Target {
    Target { host: "127.0.0.1".into(), port: sshd.port, user: user.into() }
}

fn root_key(sshd: &Sshd) -> Auth {
    Auth::Key { text: std::fs::read_to_string(sshd.dir.join("client_ed25519")).unwrap(), passphrase: None }
}

#[tokio::test]
#[ignore]
async fn key_login_as_root_runs_commands_and_pins_the_host_key() {
    let sshd = start_sshd();
    let s = Session::connect(&target(&sshd, "root"), &root_key(&sshd), None).await.unwrap();
    assert_eq!(s.host_key(), host_fingerprint(&sshd.dir));

    let out = s.run("echo out; echo err >&2; exit 3").await.unwrap();
    assert_eq!((out.status, out.stdout.as_str(), out.stderr.as_str()), (3, "out", "err"));
    assert_eq!(s.run_root("id -u").await.unwrap().stdout, "0");

    let mut lines = Vec::new();
    s.run_root_with("printf 'a\\nb\\n'; printf '{\"ok\":true}\\n'", |l| lines.push(l.to_string())).await.unwrap();
    assert_eq!(lines, vec!["a", "b", "{\"ok\":true}"]);
    s.close().await;

    // The remembered key is accepted, another one is refused.
    let fp = host_fingerprint(&sshd.dir);
    Session::connect(&target(&sshd, "root"), &root_key(&sshd), Some(&fp)).await.unwrap().close().await;
    let err = Session::connect(&target(&sshd, "root"), &root_key(&sshd), Some("SHA256:somethingelse"))
        .await
        .err()
        .unwrap();
    assert!(err.chain().any(|e| e.downcast_ref::<HostKeyChanged>().is_some()), "{err:#}");
}

#[tokio::test]
#[ignore]
async fn password_login_runs_as_root_through_sudo() {
    let sshd = start_sshd();
    let t = target(&sshd, USER);
    let s = Session::connect(&t, &Auth::Password(PASSWORD.into()), None).await.unwrap();
    assert_eq!(s.run("id -un").await.unwrap().stdout, USER);
    let root = s.run_root("id -u; echo \"$0\"").await.unwrap();
    assert_eq!(root.stdout.lines().next(), Some("0"), "{root:?}");
    assert!(!root.stdout.contains(PASSWORD) && !root.stderr.contains(PASSWORD));
    s.close().await;

    let err = Session::connect(&t, &Auth::Password("wrong".into()), None).await.err().unwrap();
    assert!(format!("{err:#}").contains("did not accept the password"), "{err:#}");
}

#[tokio::test]
#[ignore]
async fn a_local_port_reaches_the_servers_loopback() {
    let sshd = start_sshd();
    // Stands in for the panel on the server's 127.0.0.1.
    let echo = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let echo_port = echo.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut c, _)) = echo.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 64];
                let n = c.read(&mut buf).await.unwrap();
                c.write_all(&buf[..n]).await.unwrap();
            });
        }
    });

    let s = std::sync::Arc::new(Session::connect(&target(&sshd, "root"), &root_key(&sshd), None).await.unwrap());
    let local = s.forward_local(echo_port).await.unwrap();
    for _ in 0..2 {
        let mut c = tokio::net::TcpStream::connect(("127.0.0.1", local)).await.unwrap();
        c.write_all(b"panel").await.unwrap();
        let mut buf = [0u8; 5];
        c.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"panel");
    }
}

/// The manager end to end: add the server, see what is installed, read and
/// change users through the real `ostp manage`. Needs the built binary:
/// `OSTP_BIN=target/debug/ostp`. It is copied to /usr/local/bin/ostp and
/// uses /etc/ostp/config.json, so run it only on a throwaway machine.
#[tokio::test]
#[ignore]
async fn the_manager_drives_a_real_ostp() {
    let Ok(bin) = std::env::var("OSTP_BIN") else {
        eprintln!("OSTP_BIN not set; skipped");
        return;
    };
    std::fs::copy(bin, "/usr/local/bin/ostp").unwrap();
    std::fs::create_dir_all("/etc/ostp").unwrap();
    std::fs::write(
        "/etc/ostp/config.json",
        r#"{"mode":"server","config_version":3,"listen":"0.0.0.0:50000","access_keys":[{"access_key":"00112233445566778899aabbccddeeff","name":"me"}]}"#,
    )
    .unwrap();
    std::fs::write("/etc/ostp/.ostp_public_ip", "198.51.100.9\n").unwrap();

    let sshd = start_sshd();
    let file = sshd.dir.join("servers.json");
    let m = ostp_ssh::manager::Manager::new(&file, Some(ostp_ssh::store::Vault::random_key())).unwrap();
    let info = m.add("test", target(&sshd, USER), Auth::Password(PASSWORD.into()), true).await.unwrap();
    assert!(info.remembered);

    // A fresh manager (the app restarted) signs in with the remembered password.
    drop(m);
    let m = ostp_ssh::manager::Manager::new(&file, None).unwrap();
    assert!(m.probe(&info.id, None).await.unwrap_err().to_string().contains("credential store"));
    let m = ostp_ssh::manager::Manager::new(&file, Some(ostp_ssh::store::Vault::random_key())).unwrap();
    assert!(m.probe(&info.id, None).await.is_err(), "another master key cannot open the password");
    let probe = m.probe(&info.id, Some(Auth::Password(PASSWORD.into()))).await.unwrap();
    assert_eq!(probe["installed"], true, "{probe}");

    let users = m.manage(&info.id, None, &["users"]).await.unwrap();
    assert_eq!(users["users"][0]["name"], "me");
    assert!(users["users"][0]["links"][0]["uri"].as_str().unwrap().contains("198.51.100.9:50000"));
    let added = m.manage(&info.id, None, &["user-add", "phone with 'quote"]).await.unwrap();
    assert_eq!(added["user"]["name"], "phone with 'quote");
    let err = m.manage(&info.id, None, &["user-remove", "nobody"]).await.unwrap_err();
    assert_eq!(err.to_string(), "no user matches nobody");
    let status = m.manage(&info.id, None, &["status"]).await.unwrap();
    assert_eq!(status["users"], 2);
    assert_eq!(m.open_panel(&info.id, None).await.unwrap_err().to_string(), "PANEL_OFF");
}
