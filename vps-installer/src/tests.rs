use super::*;
use std::collections::BTreeMap;

#[derive(Clone)]
struct Node {
    meta: Metadata,
    bytes: Vec<u8>,
}
struct MemoryHost {
    nodes: BTreeMap<String, Node>,
    uid: u32,
    calls: Vec<String>,
    mutations: Vec<String>,
    passwords: Vec<String>,
    user: bool,
    masked: bool,
    service_active: bool,
    occupied: bool,
    fail: Option<String>,
    changed_ssh: bool,
    busy_lock: bool,
    admin_changed: bool,
    stage_created: bool,
    lock_held: bool,
    cleanup_failed: bool,
    missing: Option<String>,
    squid_race: Option<&'static str>,
    external_squid: bool,
    external_package: bool,
    ssh_mode_race: bool,
    security_issue: Option<&'static str>,
    security_race: bool,
    fingerprint_failure_after_commit: bool,
    exchange_conflict: bool,
    exchange_probe_failure: bool,
    hostkeys: Vec<String>,
    mutate_host_public_before_fingerprint: bool,
    fingerprint_snapshots: Vec<Vec<u8>>,
    mutate_public_input_before_validation: bool,
    public_input_snapshots: Vec<Vec<u8>>,
}

impl MemoryHost {
    fn new() -> Self {
        let mut host = Self {
            nodes: BTreeMap::new(),
            uid: 0,
            calls: Vec::new(),
            mutations: Vec::new(),
            passwords: vec!["synthetic-fixture-password-0123456789".into(); 2],
            user: false,
            masked: false,
            service_active: true,
            occupied: false,
            fail: None,
            changed_ssh: false,
            busy_lock: false,
            admin_changed: false,
            stage_created: false,
            lock_held: false,
            cleanup_failed: false,
            missing: None,
            squid_race: None,
            external_squid: false,
            external_package: false,
            ssh_mode_race: false,
            security_issue: None,
            security_race: false,
            fingerprint_failure_after_commit: false,
            exchange_conflict: false,
            exchange_probe_failure: false,
            hostkeys: vec!["/etc/ssh/ssh_host_ed25519_key".into()],
            mutate_host_public_before_fingerprint: false,
            fingerprint_snapshots: Vec::new(),
            mutate_public_input_before_validation: false,
            public_input_snapshots: Vec::new(),
        };
        host.file("/etc/os-release", b"ID=debian\nVERSION_ID=\"12\"\n", 0o644);
        host.file(
            SSH,
            b"Port 22\nInclude /etc/ssh/sshd_config.d/*.conf\n",
            0o644,
        );
        host.file(
            "/fixture/key.pub",
            b"ssh-ed25519 AAAAFIXTURE public-comment\n",
            0o644,
        );
        host.file(
            "/etc/ssh/ssh_host_ed25519_key",
            b"synthetic host private key",
            0o600,
        );
        host.file(
            "/etc/ssh/ssh_host_ed25519_key.pub",
            b"ssh-ed25519 HOSTKEY host-public-comment\n",
            0o644,
        );
        host.file("/usr/lib/squid/basic_ncsa_auth", b"fixture", 0o755);
        host.directory("/etc", 0o755);
        host.directory("/etc/ssh", 0o755);
        host.directory("/etc/ssh/sshd_config.d", 0o755);
        host
    }
    fn file(&mut self, path: &str, bytes: &[u8], mode: u32) {
        self.nodes.insert(
            path.into(),
            Node {
                meta: Metadata {
                    directory: false,
                    file: true,
                    uid: 0,
                    gid: 0,
                    mode,
                    links: 1,
                },
                bytes: bytes.to_vec(),
            },
        );
    }
    fn directory(&mut self, path: &str, mode: u32) {
        self.nodes.insert(
            path.into(),
            Node {
                meta: Metadata {
                    directory: true,
                    file: false,
                    uid: 0,
                    gid: 0,
                    mode,
                    links: 1,
                },
                bytes: Vec::new(),
            },
        );
    }
    fn effective(&self, protected: bool) -> String {
        let mut value = String::from("disableforwarding no\ntrustedusercakeys none\n");
        value.push_str("hostkeyagent none\n");
        for path in &self.hostkeys {
            value.push_str(&format!("hostkey {path}\n"));
        }
        if protected {
            value.push_str("authenticationmethods publickey\npubkeyauthentication yes\nauthorizedkeysfile .ssh/authorized_keys\nauthorizedkeyscommand none\npasswordauthentication no\nkbdinteractiveauthentication no\nclientaliveinterval 15\nclientalivecountmax 3\nallowtcpforwarding remote\nallowstreamlocalforwarding no\ngatewayports no\npermitlisten 127.0.0.1:43129\npermitopen none\nallowagentforwarding no\nx11forwarding no\npermittty no\nforcecommand /usr/sbin/nologin\n");
        } else {
            value.push_str("allowtcpforwarding yes\n");
        }
        value
    }
    fn symlink(&mut self, path: &str) {
        self.nodes.insert(
            path.into(),
            Node {
                meta: Metadata {
                    directory: false,
                    file: false,
                    uid: 0,
                    gid: 0,
                    mode: 0o777,
                    links: 1,
                },
                bytes: Vec::new(),
            },
        );
    }
    fn clean(&self) -> bool {
        !self
            .nodes
            .keys()
            .any(|p| p.starts_with("/tmp/plumivox-proxy.fixture"))
    }
    fn refresh_manifest(&mut self) {
        let mut value = String::new();
        for path in managed_paths() {
            value.push_str(&format!("{}  {path}\n", digest(&self.nodes[path].bytes)));
        }
        self.file(MANIFEST, value.as_bytes(), 0o600);
    }
}

impl Host for MemoryHost {
    fn uid(&self) -> u32 {
        self.uid
    }
    fn os_release(&self) -> Result<String> {
        text(self, "/etc/os-release")
    }
    fn ssh_security(&self) -> Result<()> {
        match self.security_issue {
            Some(issue) => Err(format!("SSH設定の{issue}を保持できないため導入しません")),
            None => Ok(()),
        }
    }
    fn metadata(&self, path: &str) -> Result<Option<Metadata>> {
        Ok(self.nodes.get(path).map(|n| n.meta))
    }
    fn read(&self, path: &str) -> Result<Vec<u8>> {
        self.nodes
            .get(path)
            .map(|n| n.bytes.clone())
            .ok_or_else(|| "fixture missing file".into())
    }
    fn entries(&self, path: &str) -> Result<Vec<String>> {
        let prefix = format!("{path}/");
        Ok(self
            .nodes
            .keys()
            .filter(|p| {
                p.strip_prefix(&prefix)
                    .is_some_and(|tail| !tail.contains('/'))
            })
            .cloned()
            .collect())
    }
    fn available(&self, program: &str) -> bool {
        self.missing.as_deref() != Some(program)
    }
    fn command(&mut self, program: &str, args: &[&str], input: Option<&[u8]>) -> Result<Output> {
        let call = format!("{program} {}", args.join(" "));
        self.calls.push(call.clone());
        let mutation = matches!(program, "apt-get" | "useradd" | "htpasswd")
            || (program == "systemctl"
                && matches!(
                    args.first(),
                    Some(&"mask" | &"reload" | &"daemon-reload" | &"enable")
                ))
            || (program == "ufw" && args.first() == Some(&"allow"));
        if mutation {
            self.mutations.push(call.clone());
        }
        if self.fail.as_ref().is_some_and(|f| call.contains(f)) {
            return Ok(Output {
                success: false,
                text: "provider-raw-secret-must-not-escape".into(),
            });
        }
        let mut success = true;
        let text = match (program, args.first().copied()) {
            ("ufw", Some("status")) => "Status: active\n".into(),
            ("ss", _) if self.occupied => "LISTEN\n".into(),
            ("getent", _) => {
                success = self.user;
                if self.user && args.first() == Some(&"passwd") {
                    format!("plumivox:x:1001:1001::{HOME}:/usr/sbin/nologin\n")
                } else {
                    String::new()
                }
            }
            ("dpkg-query", _) if self.external_package => "install ok installed".into(),
            ("dpkg-query", _) => {
                success = false;
                String::new()
            }
            ("sshd", Some("-T")) => {
                let admin = args
                    .iter()
                    .any(|a| a.starts_with("user=plumivox-admin-check"));
                let candidate = args.contains(&"-f");
                let installed = self.nodes[SSH]
                    .bytes
                    .windows(SSH_CONF.len())
                    .any(|w| w == SSH_CONF.as_bytes());
                if admin && candidate && self.admin_changed {
                    "changed-admin\n".into()
                } else {
                    self.effective(!admin && (candidate || installed))
                }
            }
            ("systemctl", Some("show")) => if self.external_squid {
                "loaded\n"
            } else if self.masked {
                "masked\n"
            } else {
                "not-found\n"
            }
            .into(),
            ("systemctl", Some("is-enabled")) => if self.masked {
                "masked\n"
            } else {
                "disabled\n"
            }
            .into(),
            ("systemctl", Some("mask")) => {
                self.masked = true;
                String::new()
            }
            ("systemctl", Some("is-active")) if args.contains(&"plumivox-proxy.service") => {
                success = self.service_active;
                String::new()
            }
            ("ssh-keygen", _) if args == ["-l", "-f", "/dev/stdin"] => {
                let snapshot = input
                    .ok_or_else(|| "public key validation must use fixed input".to_string())?;
                self.public_input_snapshots.push(snapshot.to_vec());
                if self.mutate_public_input_before_validation {
                    self.file(
                        "/fixture/key.pub",
                        b"ssh-ed25519 CHANGED_AFTER_SNAPSHOT\n",
                        0o644,
                    );
                }
                "256 SHA256:fixture (ED25519)\n".into()
            }
            ("ssh-keygen", _) if args.contains(&"-y") => "ssh-ed25519 HOSTKEY\n".into(),
            ("ssh-keygen", _)
                if args.contains(&"-E")
                    && self.fingerprint_failure_after_commit
                    && self.nodes.contains_key(MANIFEST) =>
            {
                success = false;
                "provider-raw-secret-must-not-escape".into()
            }
            ("ssh-keygen", _) if args.contains(&"-E") => {
                let snapshot =
                    input.ok_or_else(|| "fingerprint must use checked snapshot".to_string())?;
                self.fingerprint_snapshots.push(snapshot.to_vec());
                if self.mutate_host_public_before_fingerprint {
                    let paths: Vec<_> = self.hostkeys.iter().map(|p| format!("{p}.pub")).collect();
                    for path in paths {
                        self.file(&path, b"ssh-ed25519 RACING_REPLACEMENT\n", 0o644);
                    }
                }
                require(
                    args.last() == Some(&"/dev/stdin"),
                    "fingerprint must not reopen a public-key path",
                )?;
                require(
                    snapshot == b"ssh-ed25519 HOSTKEY\n",
                    "fingerprint did not use verified public bytes",
                )?;
                "256 SHA256:fixture public-comment (ED25519)\n".into()
            }
            ("ssh-keygen", _) => "256 SHA256:fixture public-comment (ED25519)\n".into(),
            ("useradd", _) => {
                self.user = true;
                String::new()
            }
            ("htpasswd", _) => {
                require(
                    input.is_some_and(|v| v.ends_with(b"\n")),
                    "fixture missing stdin",
                )?;
                self.file(args[3], b"plumivox:$2y$synthetic-fixture-hash\n", 0o600);
                String::new()
            }
            ("systemd-analyze", _) if self.changed_ssh => {
                self.file(SSH, b"changed concurrently\n", 0o644);
                String::new()
            }
            _ => String::new(),
        };
        Ok(Output { success, text })
    }
    fn candidate(&mut self, _: &[u8]) -> Result<String> {
        Ok("/proc/parent-fixture/fd/fixture".into())
    }
    fn lock(&mut self) -> Result<()> {
        self.mutations.push("lock".into());
        require(!self.busy_lock, "別の導入が実行中です")?;
        self.lock_held = true;
        Ok(())
    }
    fn stage(&mut self) -> Result<String> {
        self.stage_created = true;
        self.directory("/tmp/plumivox-proxy.fixture", 0o700);
        Ok("/tmp/plumivox-proxy.fixture".into())
    }
    fn probe_exchange(&mut self) -> Result<()> {
        self.mutations.push("exchange probe".into());
        require(
            !self.exchange_probe_failure,
            "このファイルシステムでRENAME_EXCHANGEを利用できません。SSH設定・サービス・パッケージは変更していません",
        )
    }
    fn write_new(&mut self, path: &str, contents: &[u8], mode: u32) -> Result<()> {
        require(
            !self.nodes.contains_key(path),
            "既存ファイルを上書きしません",
        )?;
        self.mutations.push(format!("write {path}"));
        self.file(path, contents, mode);
        Ok(())
    }
    fn mkdir_new(&mut self, path: &str, mode: u32) -> Result<()> {
        require(
            !self.nodes.contains_key(path),
            "既存ディレクトリを上書きしません",
        )?;
        self.mutations.push(format!("mkdir {path}"));
        self.directory(path, mode);
        Ok(())
    }
    fn replace_ssh(&mut self, before: &[u8], contents: &[u8]) -> Result<String> {
        verify_ssh_replacement(self)?;
        require(
            self.nodes[SSH].bytes == before,
            "保存直前にSSH設定が変更されました",
        )?;
        let mode = self.nodes[SSH].meta.mode;
        let directory = "/etc/ssh/.plumivox-sshd-backup.fixture";
        let backup = "/etc/ssh/.plumivox-sshd-backup.fixture/sshd_config.before";
        self.directory(directory, 0o700);
        if self.exchange_conflict {
            self.file(backup, b"concurrent administrator data\n", mode);
            self.file(SSH, contents, mode);
            return Err(format!(
                "SSH設定の置換状態不明。退避先: {backup}。reloadは実行していません。自動復元は行っていません"
            ));
        }
        self.mutations.push("replace ssh".into());
        self.file(backup, before, mode);
        self.file(SSH, contents, mode);
        Ok(backup.into())
    }
    fn password(&mut self, _: &str) -> Result<Zeroizing<String>> {
        require(!self.passwords.is_empty(), "対話端末が必要です")?;
        if self.passwords.len() == 1 {
            match self.squid_race {
                Some("service") => self.external_squid = true,
                Some("package") => self.external_package = true,
                Some("/etc/squid" | "/var/log/squid" | "/var/spool/squid") => {
                    self.directory(self.squid_race.unwrap(), 0o755)
                }
                _ => (),
            }
            if self.ssh_mode_race {
                self.nodes.get_mut(SSH).unwrap().meta.mode = 0o666;
            }
            if self.security_race {
                self.security_issue = Some("default ACL");
            }
        }
        Ok(Zeroizing::new(self.passwords.remove(0)))
    }
    fn wait_service(&mut self) {}
    fn cleanup(&mut self) -> Result<()> {
        require(
            !self.cleanup_failed,
            "導入用一時ディレクトリの清掃に失敗しました",
        )?;
        self.nodes
            .retain(|p, _| !p.starts_with("/tmp/plumivox-proxy.fixture"));
        Ok(())
    }
}

fn opts() -> Options {
    Options::parse(["--public-key-file", "/fixture/key.pub"].map(str::to_string))
        .unwrap()
        .unwrap()
}

#[test]
fn cli_rejects_credentials_and_invalid_values() {
    for args in [
        vec!["--password", "secret"],
        vec!["--public-port"],
        vec!["--public-port", "65536"],
        vec!["--public-port", "03128"],
        vec!["--public-port", "43129"],
        vec!["--ssh-user", "a;id"],
        vec!["--proxy-user", "-root"],
        vec!["--check"],
    ] {
        assert!(Options::parse(args.into_iter().map(str::to_string)).is_err());
    }
    assert!(Options::parse(["--help".into()]).unwrap().is_none());
    assert!(HELP.contains("--check") && HELP.contains("パスワード"));
}

#[test]
fn check_is_read_only_and_returns_fingerprints_without_key_comment() {
    let mut host = MemoryHost::new();
    let original = host.nodes.clone();
    let mut options = opts();
    options.check = true;
    let result = execute(&mut host, &options).unwrap();
    assert!(result.contains("事前確認合格") && result.contains("SHA256:fixture (ED25519)"));
    assert!(result.contains("交換機能は未検査"));
    assert!(!result.contains("public-comment"));
    assert!(host.mutations.is_empty() && !host.stage_created && !host.lock_held);
    assert_eq!(host.nodes.len(), original.len());
    for (path, node) in original {
        assert_eq!(host.nodes[&path].bytes, node.bytes);
    }
}

#[test]
fn install_then_identical_rerun_preserves_credentials_and_state() {
    let mut host = MemoryHost::new();
    let before = host.nodes[SSH].bytes.clone();
    assert!(execute(&mut host, &opts()).unwrap().contains("導入完了"));
    assert_eq!(host.nodes[BEFORE].bytes, before);
    assert!(host.masked && host.user && host.clean());
    let passwd = host.nodes["/etc/plumivox-proxy/passwd"].bytes.clone();
    let manifest = host.nodes[MANIFEST].bytes.clone();
    let calls = host.calls.join("\n");
    assert!(!calls.contains("synthetic-fixture-password"));
    let mask = host
        .calls
        .iter()
        .position(|c| c == "systemctl mask squid.service")
        .unwrap();
    let apt = host
        .calls
        .iter()
        .position(|c| c.starts_with("apt-get"))
        .unwrap();
    assert!(mask < apt);
    host.mutations.clear();
    host.passwords.clear();
    assert!(
        execute(&mut host, &opts())
            .unwrap()
            .contains("既存導入は同一")
    );
    assert!(host.mutations.is_empty());
    assert_eq!(host.nodes[MANIFEST].bytes, manifest);
    assert_eq!(host.nodes["/etc/plumivox-proxy/passwd"].bytes, passwd);
}

#[test]
fn managed_changes_are_never_repaired_or_overwritten() {
    for kind in [
        "hash",
        "mode",
        "owner",
        "extra",
        "symlink",
        "options",
        "key",
        "stopped",
        "manifest-path",
        "manifest-count",
        "new-template",
    ] {
        let mut host = MemoryHost::new();
        execute(&mut host, &opts()).unwrap();
        let mut options = opts();
        match kind {
            "hash" => host.file("/etc/plumivox-proxy/options", b"changed", 0o600),
            "mode" => host.nodes.get_mut(KEY).unwrap().meta.mode = 0o666,
            "owner" => host.nodes.get_mut(BASE).unwrap().meta.uid = 1000,
            "extra" => host.file("/etc/plumivox-proxy/.unknown", b"unknown", 0o600),
            "symlink" => host.symlink(KEY),
            "options" => options.proxy_user = "changed".into(),
            "key" => host.file("/fixture/key.pub", b"ssh-ed25519 DIFFERENT\n", 0o644),
            "stopped" => host.service_active = false,
            "manifest-path" => host.file(MANIFEST, b"0000  /unmanaged\n", 0o600),
            "manifest-count" => {
                let mut bytes = host.nodes[MANIFEST].bytes.clone();
                bytes.extend_from_slice(b"extra\n");
                host.file(MANIFEST, &bytes, 0o600);
            }
            "new-template" => {
                host.file(UNIT, b"old service\n", 0o644);
                host.refresh_manifest();
            }
            _ => unreachable!(),
        }
        host.mutations.clear();
        let original = host.nodes.clone();
        assert!(execute(&mut host, &options).is_err(), "{kind}");
        assert!(host.mutations.is_empty(), "{kind}");
        for (path, node) in original {
            assert_eq!(host.nodes[&path].bytes, node.bytes, "{kind}");
        }
    }
}

#[test]
fn preflight_refuses_unsafe_existing_state_without_mutation() {
    for kind in [
        "nonroot",
        "old-os",
        "unsupported-os",
        "user",
        "squid",
        "port",
        "match",
        "include",
        "dropin",
        "key-symlink",
        "base-symlink",
        "missing-command",
        "ssh-parse",
        "ufw",
        "admin-change",
    ] {
        let mut host = MemoryHost::new();
        match kind {
            "nonroot" => host.uid = 1000,
            "old-os" => host.file("/etc/os-release", b"ID=ubuntu\nVERSION_ID=20.04\n", 0o644),
            "unsupported-os" => host.file("/etc/os-release", b"ID=fedora\nVERSION_ID=42\n", 0o644),
            "user" => host.user = true,
            "squid" => host.directory("/etc/squid", 0o755),
            "port" => host.occupied = true,
            "match" => host.file(SSH, b"Match Address 192.0.2.1\n", 0o644),
            "include" => host.file(SSH, b"Include /custom\n", 0o644),
            "dropin" => host.file("/etc/ssh/sshd_config.d/unsafe.conf", b"Match all\n", 0o644),
            "key-symlink" => host.symlink("/fixture/key.pub"),
            "base-symlink" => host.symlink(BASE),
            "missing-command" => host.missing = Some("ufw".into()),
            "ssh-parse" => host.fail = Some("sshd -t".into()),
            "ufw" => host.fail = Some("ufw status".into()),
            "admin-change" => host.admin_changed = true,
            _ => unreachable!(),
        }
        assert!(execute(&mut host, &opts()).is_err(), "{kind}");
        assert!(host.mutations.is_empty(), "{kind}");
    }
}

#[test]
fn password_failure_does_not_reach_package_mutations() {
    for passwords in [
        vec![],
        vec!["short", "short"],
        vec!["synthetic-fixture-password-0123456789", "different"],
        vec![
            "synthetic-fixture-password-01234 6789",
            "synthetic-fixture-password-01234 6789",
        ],
        vec![
            "synthetic-fixture-password-0123456789\n",
            "synthetic-fixture-password-0123456789\n",
        ],
    ] {
        let mut host = MemoryHost::new();
        host.passwords = passwords.into_iter().map(str::to_string).collect();
        assert!(execute(&mut host, &opts()).is_err());
        assert!(
            !host
                .calls
                .iter()
                .any(|c| c.starts_with("apt-get") || c.starts_with("systemctl mask"))
        );
        assert!(!host.nodes.contains_key(BASE) && host.clean());
    }
}

#[test]
fn install_failures_preserve_mask_never_publish_manifest_or_retry() {
    for fail in [
        "apt-get update",
        "apt-get install",
        "htpasswd",
        "/usr/sbin/squid -k parse",
        "systemd-analyze verify",
        "useradd",
        "systemctl reload",
        "systemctl daemon-reload",
        "systemctl enable",
        "ufw allow",
    ] {
        let mut host = MemoryHost::new();
        host.fail = Some(fail.into());
        let error = execute(&mut host, &opts()).unwrap_err();
        assert!(!error.contains("provider-raw-secret"), "{fail}");
        assert!(host.masked && host.clean(), "{fail}");
        assert!(!host.nodes.contains_key(MANIFEST), "{fail}");
        assert_eq!(
            host.calls.iter().filter(|c| c.contains(fail)).count(),
            1,
            "{fail}"
        );
        if host.nodes.contains_key(BASE) {
            host.mutations.clear();
            assert!(execute(&mut host, &opts()).is_err());
            assert!(host.mutations.is_empty());
        }
    }
}

#[test]
fn concurrent_changes_and_lock_contention_stop_before_commit() {
    let mut host = MemoryHost::new();
    host.changed_ssh = true;
    assert!(
        execute(&mut host, &opts())
            .unwrap_err()
            .contains("SSH設定が変更")
    );
    assert!(!host.nodes.contains_key(BASE) && host.clean() && host.masked);
    assert!(
        !host
            .calls
            .iter()
            .any(|c| c.starts_with("useradd") || c.starts_with("systemctl reload"))
    );
    let mut host = MemoryHost::new();
    host.busy_lock = true;
    assert!(
        execute(&mut host, &opts())
            .unwrap_err()
            .contains("別の導入")
    );
    assert!(!host.stage_created && !host.masked && !host.nodes.contains_key(BASE));
}

#[test]
fn squid_installed_during_password_prompt_is_not_changed() {
    for race in [
        "service",
        "package",
        "/etc/squid",
        "/var/log/squid",
        "/var/spool/squid",
    ] {
        let mut host = MemoryHost::new();
        host.squid_race = Some(race);
        assert!(execute(&mut host, &opts()).is_err(), "{race}");
        assert!(
            !host.calls.iter().any(|c| c.starts_with("systemctl mask")
                || c.starts_with("apt-get")
                || c.starts_with("useradd")
                || c.starts_with("ufw allow")),
            "{race}"
        );
        assert!(
            !host.masked && !host.user && !host.nodes.contains_key(BASE) && host.clean(),
            "{race}"
        );
        if race.starts_with('/') {
            assert!(host.nodes.contains_key(race));
        }
    }
}

#[test]
fn unsafe_ssh_metadata_is_rejected_before_mutating_commands() {
    for condition in ["uid", "gid", "links", "mode", "parent", "parent-link"] {
        let mut host = MemoryHost::new();
        match condition {
            "uid" => host.nodes.get_mut(SSH).unwrap().meta.uid = 1000,
            "gid" => host.nodes.get_mut(SSH).unwrap().meta.gid = 1000,
            "links" => host.nodes.get_mut(SSH).unwrap().meta.links = 2,
            "mode" => host.nodes.get_mut(SSH).unwrap().meta.mode = 0o666,
            "parent" => host.nodes.get_mut("/etc/ssh").unwrap().meta.mode = 0o777,
            "parent-link" => host.symlink("/etc/ssh"),
            _ => unreachable!(),
        }
        assert!(execute(&mut host, &opts()).is_err(), "{condition}");
        assert!(host.mutations.is_empty(), "{condition}");
    }
    let mut host = MemoryHost::new();
    host.ssh_mode_race = true;
    assert!(execute(&mut host, &opts()).is_err());
    assert!(
        !host
            .calls
            .iter()
            .any(|c| c.starts_with("systemctl mask") || c.starts_with("apt-get"))
    );
    assert!(!host.nodes.contains_key(BASE) && host.clean());
}

#[test]
fn ssh_private_permissions_are_preserved_by_installation() {
    let mut host = MemoryHost::new();
    host.nodes.get_mut(SSH).unwrap().meta.mode = 0o600;
    execute(&mut host, &opts()).unwrap();
    assert_eq!(host.nodes[SSH].meta.mode, 0o600);
    host.mutations.clear();
    execute(&mut host, &opts()).unwrap();
    assert!(host.mutations.is_empty());
}

#[test]
fn unsupported_ssh_security_attributes_refuse_before_mutation() {
    for issue in [
        "access ACL",
        "default ACL",
        "xattr",
        "SELinux label",
        "xattr unsupported filesystem",
        "xattr inspection permission",
        "missing capability",
        "noninitial user namespace",
        "mount identity mismatch",
        "inode flags unsupported",
        "creation parent default ACL",
    ] {
        let mut host = MemoryHost::new();
        host.security_issue = Some(issue);
        let error = execute(&mut host, &opts()).unwrap_err();
        assert!(error.contains(issue));
        assert!(host.mutations.is_empty());
    }
    let mut host = MemoryHost::new();
    host.security_race = true;
    assert!(execute(&mut host, &opts()).is_err());
    assert!(!host.masked && !host.user && !host.nodes.contains_key(BASE) && host.clean());
}

#[test]
fn existing_unchanged_installation_does_not_require_replacement_capabilities() {
    for issue in [
        "access ACL",
        "default ACL",
        "xattr",
        "SELinux label",
        "missing capability",
        "noninitial user namespace",
        "mount identity mismatch",
        "inode flags unsupported",
        "creation parent default ACL",
    ] {
        let mut host = MemoryHost::new();
        let result = execute(&mut host, &opts()).unwrap();
        assert!(result.contains("root限定の退避先"));
        assert!(result.contains("旧ファイルを開いた処理を終了"));
        let backup = host.nodes["/etc/ssh/.plumivox-sshd-backup.fixture/sshd_config.before"]
            .bytes
            .clone();
        host.security_issue = Some(issue);
        host.mutations.clear();
        host.passwords.clear();
        let mut options = opts();
        options.check = true;
        assert!(
            execute(&mut host, &options)
                .unwrap()
                .contains("既存導入は同一")
        );
        assert!(host.mutations.is_empty());
        assert_eq!(
            host.nodes["/etc/ssh/.plumivox-sshd-backup.fixture/sshd_config.before"].bytes,
            backup
        );
        assert_eq!(
            host.nodes["/etc/ssh/.plumivox-sshd-backup.fixture"]
                .meta
                .mode,
            0o700
        );
    }
}

#[test]
fn missing_fingerprint_refuses_before_mutation_and_post_commit_failure_keeps_status() {
    let mut host = MemoryHost::new();
    host.nodes.remove("/etc/ssh/ssh_host_ed25519_key.pub");
    host.nodes.remove("/etc/ssh/ssh_host_ed25519_key");
    assert!(
        execute(&mut host, &opts())
            .unwrap_err()
            .contains("指紋が0件")
    );
    assert!(host.mutations.is_empty());
    let mut host = MemoryHost::new();
    host.fingerprint_failure_after_commit = true;
    let result = execute(&mut host, &opts()).unwrap();
    assert!(result.contains("導入済みですが") && result.contains("事前確認時の指紋"));
    assert!(result.contains("SHA256:fixture") && !result.contains("provider-raw-secret"));
    assert!(host.nodes.contains_key(MANIFEST));
    assert_eq!(
        host.calls
            .iter()
            .filter(|c| c.starts_with("useradd"))
            .count(),
        1
    );
}

#[test]
fn fingerprints_only_use_effective_hostkeys_and_verify_their_public_part() {
    let mut host = MemoryHost::new();
    host.hostkeys = vec!["/custom/active_host_key".into()];
    host.file("/custom/active_host_key", b"synthetic active key", 0o600);
    host.file(
        "/custom/active_host_key.pub",
        b"ssh-ed25519 HOSTKEY custom-comment\n",
        0o644,
    );
    let mut options = opts();
    options.check = true;
    let result = execute(&mut host, &options).unwrap();
    assert!(result.contains("SHA256:fixture"));
    assert!(
        host.calls
            .iter()
            .any(|call| call == "ssh-keygen -y -P  -f /custom/active_host_key")
    );
    assert!(
        host.calls
            .iter()
            .any(|call| call == "ssh-keygen -l -E sha256 -f /dev/stdin")
    );
    assert!(
        !host
            .calls
            .iter()
            .any(|call| call.contains("-E sha256 -f /etc/ssh/ssh_host_ed25519_key.pub"))
    );
    assert!(host.mutations.is_empty());
    host.file(
        "/custom/active_host_key.pub",
        b"ssh-ed25519 STALE_PUBLIC_KEY custom-comment\n",
        0o644,
    );
    let error = execute(&mut host, &options).unwrap_err();
    assert!(error.contains("一致しません") && !error.contains("STALE_PUBLIC_KEY"));
    assert!(host.mutations.is_empty());
}

#[test]
fn public_input_key_uses_one_snapshot_for_validation_and_saved_authorized_key() {
    let mut host = MemoryHost::new();
    host.mutate_public_input_before_validation = true;
    execute(&mut host, &opts()).unwrap();
    assert_eq!(
        host.public_input_snapshots,
        vec![b"ssh-ed25519 AAAAFIXTURE\n".to_vec()]
    );
    assert!(
        text(&host, KEY)
            .unwrap()
            .contains("ssh-ed25519 AAAAFIXTURE\n")
    );
    assert!(!text(&host, KEY).unwrap().contains("CHANGED_AFTER_SNAPSHOT"));
}

#[test]
fn fingerprint_uses_verified_snapshot_when_public_file_changes_after_comparison() {
    let mut host = MemoryHost::new();
    host.mutate_host_public_before_fingerprint = true;
    let mut options = opts();
    options.check = true;
    let result = execute(&mut host, &options).unwrap();
    assert!(result.contains("SHA256:fixture"));
    assert_eq!(
        host.fingerprint_snapshots,
        vec![b"ssh-ed25519 HOSTKEY\n".to_vec()]
    );
    assert_eq!(
        host.nodes["/etc/ssh/ssh_host_ed25519_key.pub"].bytes,
        b"ssh-ed25519 RACING_REPLACEMENT\n"
    );
    assert!(host.mutations.is_empty());
}

#[test]
fn unsupported_exchange_is_detected_before_packages_or_settings_are_changed() {
    let mut host = MemoryHost::new();
    host.exchange_probe_failure = true;
    let before = host.nodes[SSH].bytes.clone();
    assert!(
        execute(&mut host, &opts())
            .unwrap_err()
            .contains("RENAME_EXCHANGE")
    );
    assert_eq!(host.nodes[SSH].bytes, before);
    assert!(!host.masked && !host.user && !host.stage_created && !host.nodes.contains_key(BASE));
    assert!(!host.calls.iter().any(|call| call.starts_with("apt-get")
        || call.starts_with("systemctl mask")
        || call.starts_with("useradd")));
    let mut options = opts();
    options.check = true;
    host.mutations.clear();
    assert!(
        execute(&mut host, &options)
            .unwrap()
            .contains("交換機能は未検査")
    );
    assert!(host.mutations.is_empty());
}

#[test]
fn exchanged_conflicting_ssh_data_is_kept_without_reload_or_rollback() {
    let mut host = MemoryHost::new();
    host.exchange_conflict = true;
    let error = execute(&mut host, &opts()).unwrap_err();
    assert!(error.contains("置換状態不明") && error.contains("reloadは実行していません"));
    assert_eq!(
        host.nodes["/etc/ssh/.plumivox-sshd-backup.fixture/sshd_config.before"].bytes,
        b"concurrent administrator data\n"
    );
    assert!(text(&host, SSH).unwrap().contains(SSH_CONF));
    assert!(
        !host
            .calls
            .iter()
            .any(|call| call.starts_with("systemctl reload"))
    );
    assert!(!host.nodes.contains_key(MANIFEST));
    assert!(host.clean());
}

#[test]
fn ssh_restrictions_and_squid_never_direct_are_preserved() {
    let options = opts();
    let mut host = MemoryHost::new();
    verify_effective(&host.effective(true), &options).unwrap();
    for setting in [
        "allowtcpforwarding remote",
        "permitlisten 127.0.0.1:43129",
        "forcecommand /usr/sbin/nologin",
        "clientaliveinterval 15",
        "passwordauthentication no",
        "disableforwarding no",
    ] {
        assert!(
            verify_effective(
                &host.effective(true).replace(setting, "changed value"),
                &options
            )
            .is_err()
        );
    }
    execute(&mut host, &options).unwrap();
    let config = text(&host, "/etc/plumivox-proxy/squid.conf").unwrap();
    for line in [
        "never_direct allow all",
        "http_access deny !authenticated",
        "http_access deny !x_domains",
        "http_access deny !tls_port",
        "http_access deny !connect_method",
        "cache_peer 127.0.0.1 parent 43129 0 no-query default name=home_egress",
    ] {
        assert!(config.lines().any(|actual| actual == line));
    }
}

#[test]
fn cleanup_error_is_not_reported_as_success() {
    let mut host = MemoryHost::new();
    host.cleanup_failed = true;
    assert!(
        execute(&mut host, &opts())
            .unwrap_err()
            .contains("清掃に失敗")
    );
    assert!(host.nodes.contains_key(MANIFEST));
}
