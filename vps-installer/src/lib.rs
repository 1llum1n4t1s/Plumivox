// 導入処理（execute 以下）は Linux でだけ呼ばれる。非Linuxの通常ビルドでは
// tests.rs の検証経路以外から参照されず dead_code 警告が大量に出るため抑止する。
// Linux とテストビルドでは属性が無効なので、未使用検出は従来どおり働く。
#![cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(test)]
mod tests;

type Result<T> = std::result::Result<T, String>;
const BASE: &str = "/etc/plumivox-proxy";
const HOME: &str = "/var/lib/plumivox-ssh";
const UNIT: &str = "/etc/systemd/system/plumivox-proxy.service";
const SSH: &str = "/etc/ssh/sshd_config";
const SSH_CONF: &str = "/etc/plumivox-proxy/ssh.conf";
const BEFORE: &str = "/etc/plumivox-proxy/sshd_config.before";
const KEY: &str = "/var/lib/plumivox-ssh/.ssh/authorized_keys";
const MANIFEST: &str = "/etc/plumivox-proxy/manifest.sha256";
const SQUID_TEMPLATE: &str = include_str!("../../setup/vps/squid.conf.in");
const SERVICE: &str = include_str!("../../setup/vps/plumivox-proxy.service");
const HELP: &str = "Plumivox VPSプロキシの導入 (Debian 12+ / Ubuntu 22.04+)\n\
sudo ./plumivox-vps-setup --public-key-file ./plumivox_ed25519.pub [オプション]\n\
  --public-port PORT    認証付き公開IPv4プロキシ (既定: 3128)\n\
  --remote-port PORT    SSH逆トンネルの127.0.0.1待受 (既定: 43129)\n\
  --ssh-user NAME       新規のSSH専用ユーザー (既定: plumivox)\n\
  --proxy-user NAME     Basic認証ユーザー (既定: plumivox)\n\
  --check               事前確認だけ。パッケージ/設定/ユーザー/一時ファイルを作らない\n\
  --help                この説明を表示\n\
公開鍵はPCで作成したEd25519の.pubファイルだけを指定します。\n\
初回導入でプロキシパスワードを非表示入力します (32文字以上)。\n\
パスワードは引数・環境変数へ渡さないでください。\n\
既に有効なUFWとSSHが必要です。SSHポート/管理者認証は変更しません。\n\
VPS提供元のファイアウォールにも公開TCPポートを手動で許可してください。\n\
既存Squid/同名ユーザー/競合ポート/変更済み設定は上書きしません。\n\
導入中は別の管理者によるSSH設定・パッケージの変更を停止してください。\n\
ACL・拡張属性・SELinuxラベルの保持を保証できない構成では新規導入を拒否します。\n\
交換したSSH原本はroot限定の一意な退避先へ保持し、自動削除しません。\n\
退避先の整理は同時管理操作と旧ファイルを開く処理の終了・両内容と動作の確認後に管理者が行います。\n";

#[derive(Clone, Debug)]
struct Options {
    public_port: u16,
    remote_port: u16,
    ssh_user: String,
    proxy_user: String,
    key_file: String,
    check: bool,
}

impl Options {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Option<Self>> {
        let mut value = Self {
            public_port: 3128,
            remote_port: 43129,
            ssh_user: "plumivox".into(),
            proxy_user: "plumivox".into(),
            key_file: String::new(),
            check: false,
        };
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--help" => return Ok(None),
                "--check" => value.check = true,
                "--public-key-file" | "--public-port" | "--remote-port" | "--ssh-user"
                | "--proxy-user" => {
                    let next = args
                        .next()
                        .ok_or_else(|| format!("{arg} の値がありません"))?;
                    match arg.as_str() {
                        "--public-key-file" => value.key_file = next,
                        "--public-port" => value.public_port = parse_port(&next)?,
                        "--remote-port" => value.remote_port = parse_port(&next)?,
                        "--ssh-user" => value.ssh_user = next,
                        _ => value.proxy_user = next,
                    }
                }
                _ => return Err("未対応の引数です (--helpで確認してください)".into()),
            }
        }
        require(
            value.public_port != value.remote_port,
            "公開ポートと逆トンネルポートを分けてください",
        )?;
        for name in [&value.ssh_user, &value.proxy_user] {
            require(
                name.len() <= 31
                    && name.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
                    && name.bytes().all(|b| {
                        b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-'
                    }),
                "ユーザー名は小文字英数字・_・-で指定してください",
            )?;
        }
        require(
            !value.key_file.is_empty(),
            "通常ファイルのEd25519公開鍵を--public-key-fileへ指定してください",
        )?;
        Ok(Some(value))
    }

    fn options(&self) -> String {
        format!(
            "format=1\npublic_port={}\nremote_port={}\nssh_user={}\nproxy_user={}\n",
            self.public_port, self.remote_port, self.ssh_user, self.proxy_user
        )
    }
    fn squid(&self) -> String {
        SQUID_TEMPLATE
            .replace("\r\n", "\n")
            .replace("@PUBLIC_PORT@", &self.public_port.to_string())
            .replace("@REMOTE_PORT@", &self.remote_port.to_string())
    }
    fn ssh(&self) -> String {
        format!(
            "# Plumivox所有: このユーザーだけに適用\nMatch User {}\n    AuthenticationMethods publickey\n    PubkeyAuthentication yes\n    AuthorizedKeysFile .ssh/authorized_keys\n    AuthorizedKeysCommand none\n    PasswordAuthentication no\n    KbdInteractiveAuthentication no\n    ClientAliveInterval 15\n    ClientAliveCountMax 3\n    AllowTcpForwarding remote\n    AllowStreamLocalForwarding no\n    GatewayPorts no\n    PermitListen 127.0.0.1:{}\n    PermitOpen none\n    AllowAgentForwarding no\n    X11Forwarding no\n    PermitTTY no\n    ForceCommand /usr/sbin/nologin\nMatch all\n",
            self.ssh_user, self.remote_port
        )
    }
    fn key(&self, key: &str) -> String {
        format!(
            "restrict,port-forwarding,permitlisten=\"127.0.0.1:{}\",command=\"/usr/sbin/nologin\" {key}\n",
            self.remote_port
        )
    }
    fn context(&self) -> String {
        format!("user={},host=localhost,addr=127.0.0.1", self.ssh_user)
    }
}

fn parse_port(value: &str) -> Result<u16> {
    require(
        !value.starts_with('0') && value.bytes().all(|b| b.is_ascii_digit()),
        "ポートは1024～65535の整数です",
    )?;
    value
        .parse::<u16>()
        .ok()
        .filter(|p| *p >= 1024)
        .ok_or_else(|| "ポートは1024～65535の整数です".into())
}

fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

#[derive(Clone, Copy, Debug)]
struct Metadata {
    directory: bool,
    file: bool,
    uid: u32,
    gid: u32,
    mode: u32,
    links: u64,
}
struct Output {
    success: bool,
    text: String,
}

// 検証はこの境界のメモリー実装だけを差し替える。実行バイナリに差し替え設定は持たせない。
trait Host {
    fn uid(&self) -> u32;
    fn os_release(&self) -> Result<String>;
    fn ssh_security(&self) -> Result<()>;
    fn metadata(&self, path: &str) -> Result<Option<Metadata>>;
    fn read(&self, path: &str) -> Result<Vec<u8>>;
    fn public_key_snapshot(&self, path: &str) -> Result<Vec<u8>> {
        self.read(path)
    }
    fn entries(&self, path: &str) -> Result<Vec<String>>;
    fn available(&self, program: &str) -> bool;
    fn command(&mut self, program: &str, args: &[&str], input: Option<&[u8]>) -> Result<Output>;
    fn candidate(&mut self, contents: &[u8]) -> Result<String>;
    fn lock(&mut self) -> Result<()>;
    fn probe_exchange(&mut self) -> Result<()>;
    fn stage(&mut self) -> Result<String>;
    fn write_new(&mut self, path: &str, contents: &[u8], mode: u32) -> Result<()>;
    fn mkdir_new(&mut self, path: &str, mode: u32) -> Result<()>;
    fn replace_ssh(&mut self, before: &[u8], contents: &[u8]) -> Result<String>;
    fn password(&mut self, prompt: &str) -> Result<Zeroizing<String>>;
    fn wait_service(&mut self);
    fn cleanup(&mut self) -> Result<()>;
}

fn output(host: &mut impl Host, program: &str, args: &[&str], message: &str) -> Result<String> {
    let out = host.command(program, args, None)?;
    require(out.success, message)?;
    Ok(out.text)
}
fn text(host: &impl Host, path: &str) -> Result<String> {
    String::from_utf8(host.read(path)?).map_err(|_| "設定ファイルの文字コードが不正です".into())
}
fn regular(host: &impl Host, path: &str, message: &str) -> Result<Metadata> {
    host.metadata(path)?
        .filter(|m| m.file)
        .ok_or_else(|| message.into())
}
fn absent(host: &impl Host, path: &str) -> Result<()> {
    require(
        host.metadata(path)?.is_none(),
        "同名の既存ファイルがあります。再実行で上書きしません",
    )
}
fn root_mode(host: &impl Host, path: &str, mode: u32, directory: bool) -> Result<()> {
    require(
        host.metadata(path)?.is_some_and(|m| {
            m.uid == 0
                && m.gid == 0
                && m.mode == mode
                && if directory { m.directory } else { m.file }
        }),
        "管理対象の所有者・権限・形式が変更されています",
    )
}
fn verify_ssh_file(host: &impl Host) -> Result<()> {
    for path in ["/etc", "/etc/ssh"] {
        require(
            host.metadata(path)?
                .is_some_and(|m| m.directory && m.uid == 0 && m.mode & 0o022 == 0),
            "SSH設定ディレクトリの所有者・権限・形式が不正です",
        )?;
    }
    let m = regular(host, SSH, "通常ファイルのsshd_configが必要です")?;
    require(
        m.uid == 0 && m.gid == 0 && m.links == 1 && m.mode & 0o7022 == 0,
        "SSH設定の所有者・権限・形式が不正です",
    )
}
fn verify_ssh_replacement(host: &impl Host) -> Result<()> {
    verify_ssh_file(host)?;
    host.ssh_security()
}
fn digest(contents: &[u8]) -> String {
    Sha256::digest(contents)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn managed_paths() -> [&'static str; 8] {
    [
        "/etc/plumivox-proxy/options",
        "/etc/plumivox-proxy/squid.conf",
        "/etc/plumivox-proxy/passwd",
        UNIT,
        SSH_CONF,
        KEY,
        SSH,
        BEFORE,
    ]
}

fn validate_os(contents: &str) -> Result<()> {
    let field = |name: &str| {
        contents
            .lines()
            .filter_map(|l| l.split_once('='))
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.trim_matches('"').trim_matches('\''))
            .unwrap_or("")
    };
    let id = field("ID");
    let version = field("VERSION_ID");
    let parts = version
        .split('.')
        .map(str::parse::<u32>)
        .collect::<std::result::Result<Vec<_>, _>>();
    let valid = parts.ok().is_some_and(|p| match id {
        "debian" => p.first().is_some_and(|major| *major >= 12),
        "ubuntu" => p.len() == 2 && (p[0], p[1]) >= (22, 4),
        _ => false,
    });
    require(valid, "Debian 12+またはUbuntu 22.04+が必要です")
}

fn validate_ssh_baseline(contents: &str, allow_standard_include: bool) -> Result<()> {
    for line in contents.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.is_empty() || fields[0].starts_with('#') {
            continue;
        }
        let keyword = fields[0].to_ascii_lowercase();
        require(
            keyword != "match"
                && (keyword != "include"
                    || (allow_standard_include
                        && fields.len() == 2
                        && fields[1] == "/etc/ssh/sshd_config.d/*.conf")),
            "既存SSHにMatchまたは独自Includeがあります。標準構成の専用VPSで実行してください",
        )?;
    }
    Ok(())
}
fn setting<'a>(effective: &'a str, key: &str) -> Option<&'a str> {
    effective.lines().find_map(|line| {
        line.split_once(' ')
            .filter(|(k, _)| *k == key)
            .map(|(_, v)| v)
    })
}
fn verify_effective(effective: &str, opts: &Options) -> Result<()> {
    let listen = format!("127.0.0.1:{}", opts.remote_port);
    for (key, expected) in [
        ("authenticationmethods", "publickey"),
        ("pubkeyauthentication", "yes"),
        ("authorizedkeysfile", ".ssh/authorized_keys"),
        ("authorizedkeyscommand", "none"),
        ("passwordauthentication", "no"),
        ("kbdinteractiveauthentication", "no"),
        ("clientaliveinterval", "15"),
        ("clientalivecountmax", "3"),
        ("allowtcpforwarding", "remote"),
        ("allowstreamlocalforwarding", "no"),
        ("gatewayports", "no"),
        ("permitlisten", listen.as_str()),
        ("permitopen", "none"),
        ("allowagentforwarding", "no"),
        ("x11forwarding", "no"),
        ("permittty", "no"),
        ("forcecommand", "/usr/sbin/nologin"),
        ("disableforwarding", "no"),
    ] {
        require(
            setting(effective, key) == Some(expected),
            "SSHの既存Match制約と専用設定が競合します。管理者が確認してください",
        )?;
    }
    Ok(())
}
fn ports_free(host: &mut impl Host, opts: &Options) -> Result<()> {
    for port in [opts.public_port, opts.remote_port] {
        let filter = format!("sport = :{port}");
        let active = output(
            host,
            "ss",
            &["-H", "-ltn", &filter],
            "TCPポートの状態を確認できません",
        )?;
        require(active.trim().is_empty(), "TCPポートは使用中です")?;
    }
    Ok(())
}
fn account_absent(host: &mut impl Host, opts: &Options) -> Result<()> {
    for database in ["passwd", "group"] {
        let found = host.command("getent", &[database, &opts.ssh_user], None)?;
        require(
            !found.success,
            "SSHユーザーまたは同名グループが既に存在します。新しい専用ユーザー名を指定してください",
        )?;
    }
    Ok(())
}
fn ufw_active(host: &mut impl Host) -> Result<()> {
    let state = output(host, "ufw", &["status"], "UFWの状態を確認できません")?;
    require(
        state.lines().next() == Some("Status: active"),
        "既存のUFWが有効である必要があります。SSH管理アクセスを許可・確認したうえで管理者が有効化してください",
    )
}

fn squid_absent(host: &mut impl Host) -> Result<()> {
    let load = output(
        host,
        "systemctl",
        &["show", "squid.service", "-p", "LoadState", "--value"],
        "Squidの状態を確認できません",
    )?;
    require(
        load.trim() == "not-found",
        "既存Squidがあります。専用VPSを使ってください",
    )?;
    for path in ["/etc/squid", "/var/log/squid", "/var/spool/squid"] {
        absent(host, path)?;
    }
    let installed = host.command("dpkg-query", &["-W", "-f=${Status}", "squid"], None)?;
    require(
        !installed.text.contains("install ok installed"),
        "既存Squidパッケージがあります。専用VPSを使ってください",
    )
}

fn verify_owned(host: &mut impl Host, opts: &Options, key: &str) -> Result<()> {
    root_mode(host, BASE, 0o755, true)?;
    root_mode(host, MANIFEST, 0o600, false)?;
    let manifest = text(host, MANIFEST)?;
    let lines: Vec<_> = manifest.lines().collect();
    let paths = managed_paths();
    require(lines.len() == paths.len(), "マニフェストが変更されています")?;
    for (path, line) in paths.into_iter().zip(lines) {
        let m = regular(host, path, "管理対象ファイルが変更されています")?;
        require(m.uid == 0, "管理対象ファイルの所有者が変更されています")?;
        let expected = format!("{}  {path}", digest(&host.read(path)?));
        require(
            line == expected,
            "管理対象ファイルが変更されています。再実行で上書きしません",
        )?;
    }
    for path in [
        "/etc/plumivox-proxy/options",
        "/etc/plumivox-proxy/passwd",
        BEFORE,
    ] {
        root_mode(host, path, 0o600, false)?;
    }
    for path in ["/etc/plumivox-proxy/squid.conf", UNIT, SSH_CONF, KEY] {
        root_mode(host, path, 0o644, false)?;
    }
    let mut names = host.entries(BASE)?;
    names.sort();
    let mut expected = [
        "options",
        "squid.conf",
        "passwd",
        "ssh.conf",
        "sshd_config.before",
        "manifest.sha256",
    ]
    .map(|n| format!("{BASE}/{n}"))
    .to_vec();
    expected.sort();
    require(
        names == expected,
        "導入ディレクトリに未管理のファイルがあります",
    )?;
    for (path, content) in [
        ("/etc/plumivox-proxy/options", opts.options()),
        ("/etc/plumivox-proxy/squid.conf", opts.squid()),
        (SSH_CONF, opts.ssh()),
        (KEY, opts.key(key)),
        (UNIT, SERVICE.replace("\r\n", "\n")),
    ] {
        require(
            host.read(path)? == content.as_bytes(),
            "導入条件・公開鍵・配布設定が変わっています。既存導入の変更は管理者が確認してください",
        )?;
    }
    let passwd = output(
        host,
        "getent",
        &["passwd", &opts.ssh_user],
        "SSHユーザーの設定が変更されています",
    )?;
    let fields: Vec<_> = passwd.trim_end().split(':').collect();
    require(
        fields.len() == 7
            && fields[0] == opts.ssh_user
            && fields[5] == HOME
            && fields[6] == "/usr/sbin/nologin",
        "SSHユーザーの設定が変更されています",
    )?;
    root_mode(host, HOME, 0o755, true)?;
    root_mode(host, "/var/lib/plumivox-ssh/.ssh", 0o755, true)
}

fn fingerprints(host: &mut impl Host) -> Result<String> {
    let mut result = "SSHホスト鍵 (SHA256):\n".to_string();
    let mut count = 0;
    let effective = output(
        host,
        "sshd",
        &["-T"],
        "SSHホスト鍵の有効設定を確認できません",
    )?;
    require(
        setting(&effective, "hostkeyagent").is_none_or(|value| value == "none"),
        "HostKeyAgentを使うSSHホスト鍵は安全に照合できないため導入しません",
    )?;
    require(
        !effective.lines().any(|line| {
            line.strip_prefix("hostcertificate ")
                .is_some_and(|value| value != "none")
        }),
        "SSHホスト証明書を使う構成は安全に照合できないため導入しません",
    )?;
    let mut keys = Vec::new();
    for line in effective.lines() {
        if let Some(path) = line.strip_prefix("hostkey ") {
            require(
                path.starts_with('/')
                    && !path.chars().any(char::is_whitespace)
                    && !path.contains('%')
                    && !path.split('/').any(|part| part == "." || part == ".."),
                "SSHホスト鍵のカスタムパスを安全に扱えないため導入しません",
            )?;
            if !keys.iter().any(|existing: &String| existing == path) {
                keys.push(path.to_owned());
            }
        }
    }
    for private in keys {
        if host.metadata(&private)?.is_none() {
            continue;
        }
        regular(
            host,
            &private,
            "SSHホスト秘密鍵の形式を安全に確認できません",
        )?;
        let path = format!("{private}.pub");
        regular(
            host,
            &path,
            "実効SSHホスト鍵に対応する公開鍵がないか安全に確認できません",
        )?;
        let public = text(host, &path)?;
        let normalize = |value: &str| -> Result<String> {
            let lines: Vec<_> = value
                .lines()
                .filter(|line| !line.trim().is_empty())
                .collect();
            require(lines.len() == 1, "SSHホスト公開鍵の形式が不正です")?;
            let fields: Vec<_> = lines[0].split_whitespace().collect();
            require(fields.len() >= 2, "SSHホスト公開鍵の形式が不正です")?;
            Ok(format!("{} {}", fields[0], fields[1]))
        };
        // .pubだけが古い場合も誤案内しない。空passphrase指定で対話認証・再試行を行わない。
        let derived = output(
            host,
            "ssh-keygen",
            &["-y", "-P", "", "-f", &private],
            "実効SSHホスト鍵と公開鍵を安全に照合できません",
        )?;
        let checked_public = normalize(&derived)?;
        require(
            normalize(&public)? == checked_public,
            "実効SSHホスト秘密鍵と公開鍵が一致しません。管理者が確認してください",
        )?;
        // 照合後のpath再読込みを避け、照合済み公開部分の固定スナップショットだけを渡す。
        let snapshot = format!("{checked_public}\n");
        let fingerprint = host.command(
            "ssh-keygen",
            &["-l", "-E", "sha256", "-f", "/dev/stdin"],
            Some(snapshot.as_bytes()),
        )?;
        require(fingerprint.success, "SSHホスト鍵の指紋を取得できません")?;
        let fingerprint = fingerprint.text;
        for line in fingerprint.lines() {
            let fields: Vec<_> = line.split_whitespace().collect();
            require(
                fields.len() >= 4 && fields[1].starts_with("SHA256:"),
                "SSHホスト鍵の指紋形式が不正です",
            )?;
            // コメントは出力しない。指紋と鍵種別だけを管理コンソールとの照合へ渡す。
            result.push_str(&format!("{} {}\n", fields[1], fields.last().unwrap_or(&"")));
            count += 1;
        }
    }
    require(
        count > 0,
        "有効なSSHホスト公開鍵の指紋が0件です。管理コンソールとの照合ができないため導入しません",
    )?;
    Ok(result)
}

fn preflight(host: &mut impl Host, opts: &Options) -> Result<(bool, String)> {
    regular(
        host,
        &opts.key_file,
        "通常ファイルのEd25519公開鍵を--public-key-fileへ指定してください",
    )?;
    require(
        host.uid() == 0,
        "sudoで実行してください (--checkもrootによる読取り確認が必要です)",
    )?;
    validate_os(&host.os_release()?)?;
    for program in [
        "systemctl",
        "systemd-analyze",
        "sshd",
        "ssh-keygen",
        "ufw",
        "ss",
        "getent",
        "useradd",
        "apt-get",
        "dpkg-query",
    ] {
        require(
            host.available(program),
            &format!(
                "必要なコマンドがありません: {program} (SSH・UFW・iproute2を管理者が準備してください)"
            ),
        )?;
    }
    output(
        host,
        "systemctl",
        &["is-active", "--quiet", "ssh.service"],
        "ssh.serviceが起動している必要があります",
    )?;
    ufw_active(host)?;
    verify_ssh_file(host)?;
    output(
        host,
        "sshd",
        &["-t"],
        "現在のsshd_configの検証に失敗しました",
    )?;
    let key_text = String::from_utf8(host.public_key_snapshot(&opts.key_file)?)
        .map_err(|_| "公開鍵の文字コードが不正です".to_string())?;
    let lines: Vec<_> = key_text.lines().filter(|l| !l.trim().is_empty()).collect();
    require(
        lines.len() == 1,
        "公開鍵は一行のEd25519公開鍵にしてください",
    )?;
    let fields: Vec<_> = lines[0].split_whitespace().collect();
    require(
        fields.len() >= 2 && fields[0] == "ssh-ed25519",
        "公開鍵は一行のEd25519公開鍵にしてください",
    )?;
    let key = format!("{} {}", fields[0], fields[1]);
    let checked = host.command(
        "ssh-keygen",
        &["-l", "-f", "/dev/stdin"],
        Some(format!("{key}\n").as_bytes()),
    )?;
    require(checked.success, "公開鍵が不正です")?;
    let owned = host.metadata(BASE)?.is_some();
    if owned {
        verify_owned(host, opts, &key)?;
    } else {
        verify_ssh_replacement(host)?;
        for path in [HOME, UNIT, SSH_CONF] {
            absent(host, path)?;
        }
        account_absent(host, opts)?;
    }
    validate_ssh_baseline(&text(host, if owned { BEFORE } else { SSH })?, true)?;
    if host.metadata("/etc/ssh/sshd_config.d")?.is_some() {
        for path in host.entries("/etc/ssh/sshd_config.d")? {
            if path.ends_with(".conf") {
                regular(host, &path, "SSH drop-inの形式を確認できません")?;
                validate_ssh_baseline(&text(host, &path)?, false)?;
            }
        }
    }
    if owned {
        let load = output(
            host,
            "systemctl",
            &["show", "squid.service", "-p", "LoadState", "--value"],
            "Squidの状態を確認できません",
        )?;
        if load.trim() != "not-found" {
            let enabled = host.command("systemctl", &["is-enabled", "squid.service"], None)?;
            require(
                enabled.text.trim() == "masked",
                "既存Squidがあります。専用VPSを使ってください",
            )?;
        }
    }
    if !owned {
        squid_absent(host)?;
        ports_free(host, opts)?;
    }
    let effective = output(
        host,
        "sshd",
        &["-T", "-C", &opts.context()],
        "SSH有効設定を確認できません",
    )?;
    require(
        setting(&effective, "disableforwarding") == Some("no"),
        "既存SSH設定が転送を禁止しています。管理者が確認してください",
    )?;
    require(
        ["allowusers", "allowgroups", "denyusers", "denygroups"]
            .iter()
            .all(|k| setting(&effective, k).is_none()),
        "SSHのユーザー/グループ制限が設定されています。専用VPSで管理者が適用範囲を確認してください",
    )?;
    require(
        setting(&effective, "trustedusercakeys") == Some("none"),
        "SSHのCA認証があります。公開鍵だけの専用ユーザーを保証できません",
    )?;
    if owned {
        verify_effective(&effective, opts)?;
        output(
            host,
            "/usr/sbin/squid",
            &["-k", "parse", "-f", "/etc/plumivox-proxy/squid.conf"],
            "Squid設定の検証に失敗しました",
        )?;
        output(
            host,
            "systemctl",
            &["is-active", "--quiet", "plumivox-proxy.service"],
            "既存サービスが停止しています。管理者が原因を確認してください",
        )?;
    } else {
        require(
            matches!(
                setting(&effective, "allowtcpforwarding"),
                Some("yes" | "all" | "remote")
            ),
            "既存SSH設定でremote forwardingが許可されていません。管理者が確認してください",
        )?;
        let mut candidate = host.read(SSH)?;
        candidate.extend_from_slice(b"\n");
        candidate.extend_from_slice(opts.ssh().as_bytes());
        let candidate = host.candidate(&candidate)?;
        output(
            host,
            "sshd",
            &["-t", "-f", &candidate],
            "候補SSH設定が不正です",
        )?;
        let effective = output(
            host,
            "sshd",
            &["-T", "-f", &candidate, "-C", &opts.context()],
            "候補SSH有効設定を確認できません",
        )?;
        verify_effective(&effective, opts)?;
        let admin = if opts.ssh_user == "plumivox-admin-check" {
            "plumivox-admin-check2"
        } else {
            "plumivox-admin-check"
        };
        let context = format!("user={admin},host=localhost,addr=127.0.0.1");
        let before = output(
            host,
            "sshd",
            &["-T", "-C", &context],
            "既存SSH有効設定を確認できません",
        )?;
        let after = output(
            host,
            "sshd",
            &["-T", "-f", &candidate, "-C", &context],
            "候補SSH有効設定を確認できません",
        )?;
        require(
            before == after,
            "既存SSHの他ユーザー設定へ影響するため導入を拒否しました",
        )?;
    }
    Ok((owned, key))
}

fn install(host: &mut impl Host, opts: &Options, key: &str) -> Result<String> {
    verify_ssh_replacement(host)?;
    host.lock()?;
    for path in [BASE, HOME, UNIT, SSH_CONF] {
        absent(host, path)?;
    }
    account_absent(host, opts)?;
    ports_free(host, opts)?;
    // 実際のSSH設定へ触れない2候補だけで、同じファイルシステムの交換機能を検査する。
    // --checkにはこの書込み検査を含めない。
    host.probe_exchange()?;
    let stage = host.stage()?;
    let before = host.read(SSH)?;
    let stage_ssh = format!("{stage}/ssh.conf");
    let mut candidate = before.clone();
    candidate.extend_from_slice(format!("\nInclude {stage_ssh}\n").as_bytes());
    for (name, contents) in [
        ("squid.conf", opts.squid()),
        ("ssh.conf", opts.ssh()),
        ("authorized_keys", opts.key(key)),
        ("options", opts.options()),
        ("plumivox-proxy.service", SERVICE.replace("\r\n", "\n")),
    ] {
        host.write_new(&format!("{stage}/{name}"), contents.as_bytes(), 0o600)?;
    }
    host.write_new(&format!("{stage}/sshd_config.before"), &before, 0o600)?;
    let stage_config = format!("{stage}/sshd_config");
    host.write_new(&stage_config, &candidate, 0o600)?;
    output(
        host,
        "sshd",
        &["-t", "-f", &stage_config],
        "候補SSH設定が不正です",
    )?;
    let effective = output(
        host,
        "sshd",
        &["-T", "-f", &stage_config, "-C", &opts.context()],
        "候補SSH有効設定を確認できません",
    )?;
    verify_effective(&effective, opts)?;
    let password = host.password("32文字以上のランダムなプロキシパスワードを入力してください: ")?;
    let confirmation = host.password("確認のため再入力: ")?;
    require(
        (32..=72).contains(&password.len())
            && *password == *confirmation
            && password.bytes().all(|b| b.is_ascii_graphic()),
        "パスワードは同一の32～72文字の半角印字可能文字で入力してください",
    )?;
    drop(confirmation);
    // 対話待機中に第三者が導入・編集した状態を、最初の管理コマンド変更前に再確認する。
    // 専用ロックはこのインストーラー間だけの排他なので外部管理者の操作を除外しない。
    verify_ssh_replacement(host)?;
    require(host.read(SSH)? == before, "準備中にSSH設定が変更されました")?;
    squid_absent(host)?;
    for path in [BASE, HOME, UNIT, SSH_CONF] {
        absent(host, path)?;
    }
    account_absent(host, opts)?;
    ufw_active(host)?;
    ports_free(host, opts)?;
    output(
        host,
        "systemctl",
        &["mask", "squid.service"],
        "Squid標準サービスの自動起動を抑止できません",
    )?;
    output(
        host,
        "apt-get",
        &["update", "-qq"],
        "パッケージ一覧の更新に失敗しました。Squidのmaskは保持します",
    )?;
    output(
        host,
        "apt-get",
        &[
            "install",
            "-y",
            "-qq",
            "--no-install-recommends",
            "squid",
            "apache2-utils",
        ],
        "パッケージの導入に失敗しました。Squidのmaskは保持します",
    )?;
    require(
        host.metadata("/usr/lib/squid/basic_ncsa_auth")?
            .is_some_and(|m| m.file && m.mode & 0o111 != 0),
        "対応するSquid認証ヘルパーがありません",
    )?;
    let mut stdin = Zeroizing::new(password.as_bytes().to_vec());
    stdin.push(b'\n');
    drop(password);
    let passwd = format!("{stage}/passwd");
    let hashed = host.command(
        "htpasswd",
        &["-i", "-B", "-c", &passwd, &opts.proxy_user],
        Some(&stdin),
    )?;
    drop(stdin);
    require(
        hashed.success,
        "プロキシパスワードのハッシュ生成に失敗しました",
    )?;
    regular(host, &passwd, "パスワードファイルの生成を確認できません")?;
    output(
        host,
        "/usr/sbin/squid",
        &["-k", "parse", "-f", &format!("{stage}/squid.conf")],
        "候補Squid設定の検証に失敗しました",
    )?;
    output(
        host,
        "systemd-analyze",
        &["verify", &format!("{stage}/plumivox-proxy.service")],
        "候補systemd設定の検証に失敗しました",
    )?;
    verify_ssh_replacement(host)?;
    require(host.read(SSH)? == before, "準備中にSSH設定が変更されました")?;
    ufw_active(host)?;
    ports_free(host, opts)?;
    for path in [BASE, HOME, UNIT, SSH_CONF] {
        absent(host, path)?;
    }
    account_absent(host, opts)?;
    host.mkdir_new(BASE, 0o755)?;
    host.mkdir_new(HOME, 0o755)?;
    host.mkdir_new("/var/lib/plumivox-ssh/.ssh", 0o755)?;
    output(
        host,
        "useradd",
        &[
            "--user-group",
            "--no-create-home",
            "--home-dir",
            HOME,
            "--shell",
            "/usr/sbin/nologin",
            "--password",
            "*",
            &opts.ssh_user,
        ],
        "専用SSHユーザーの作成に失敗しました",
    )?;
    for (name, target, mode) in [
        ("passwd", "/etc/plumivox-proxy/passwd", 0o600),
        ("squid.conf", "/etc/plumivox-proxy/squid.conf", 0o644),
        ("options", "/etc/plumivox-proxy/options", 0o600),
        ("authorized_keys", KEY, 0o644),
        ("plumivox-proxy.service", UNIT, 0o644),
        ("ssh.conf", SSH_CONF, 0o644),
        ("sshd_config.before", BEFORE, 0o600),
    ] {
        let bytes = host.read(&format!("{stage}/{name}"))?;
        host.write_new(target, &bytes, mode)?;
        root_mode(host, target, mode, false)?;
    }
    let mut final_ssh = before.clone();
    final_ssh.extend_from_slice(format!("\nInclude {SSH_CONF}\n").as_bytes());
    let retained_backup = host.replace_ssh(&before, &final_ssh)?;
    let finish = (|| -> Result<()> {
        output(
            host,
            "sshd",
            &["-t"],
            "実際のSSH設定の検証に失敗しました (reloadは実行していません)",
        )?;
        let effective = output(
            host,
            "sshd",
            &["-T", "-C", &opts.context()],
            "実際のSSH有効設定を確認できません (reloadは実行していません)",
        )?;
        verify_effective(&effective, opts)?;
        output(
            host,
            "systemctl",
            &["reload", "ssh.service"],
            "SSH設定のreloadに失敗しました",
        )?;
        output(
            host,
            "systemctl",
            &["daemon-reload"],
            "systemd設定の再読込みに失敗しました",
        )?;
        output(
            host,
            "systemctl",
            &["enable", "--now", "plumivox-proxy.service"],
            "プロキシの起動に失敗しました",
        )?;
        host.wait_service();
        output(
            host,
            "systemctl",
            &["is-active", "--quiet", "plumivox-proxy.service"],
            "プロキシ起動を確認できません。管理者が原因を確認してください",
        )?;
        output(
            host,
            "ufw",
            &[
                "allow",
                "proto",
                "tcp",
                "from",
                "0.0.0.0/0",
                "to",
                "any",
                "port",
                &opts.public_port.to_string(),
                "comment",
                "plumivox-proxy",
            ],
            "UFW公開ポートの追加に失敗しました",
        )?;
        let mut manifest = String::new();
        for path in managed_paths() {
            manifest.push_str(&format!("{}  {path}\n", digest(&host.read(path)?)));
        }
        host.write_new(MANIFEST, manifest.as_bytes(), 0o600)?;
        root_mode(host, MANIFEST, 0o600, false)
    })();
    finish.map_err(|message| format!("{message}\nSSH設定の旧inodeの退避先: {retained_backup}。自動削除・自動復元は行っていません"))?;
    Ok(retained_backup)
}

fn execute(host: &mut impl Host, opts: &Options) -> Result<String> {
    let (owned, key) = preflight(host, opts)?;
    // 導入前に照合用の情報を確保し、公開鍵が0件・読取り不能なら変更を始めない。
    let checked_fingerprints = fingerprints(host)?;
    let installing = !owned && !opts.check;
    let mut message = if owned {
        "既存導入は同一です。パスワード・公開鍵・設定を保持しました。\n".to_string()
    } else if opts.check {
        "事前確認合格。交換機能は未検査です (--checkでは書込み検査を行いません)。導入時にSquid/apache2-utilsを追加し、候補設定を検証します。\n".to_string()
    } else {
        let result = install(host, opts, &key);
        let cleanup = host.cleanup();
        let retained_backup = result?;
        cleanup.map_err(|_| format!("導入済みですが、一時ディレクトリの清掃に失敗しました。SSH設定の旧inodeの退避先: {retained_backup}。管理者が確認してください"))?;
        format!(
            "導入完了: 公開TCP {} / 逆トンネル127.0.0.1:{} / SSHユーザー {} / 認証ユーザー {}\nVPS提供元のファイアウォールで公開TCP {}を手動で許可してください。\nSSH設定の旧inodeをroot限定の退避先へ保持しました: {retained_backup}\n退避先は自動削除しません。同時の管理操作と旧ファイルを開いた処理を終了し、現在と退避先の内容・SSHの動作を確認した管理者が手動で整理してください。\n",
            opts.public_port, opts.remote_port, opts.ssh_user, opts.proxy_user, opts.public_port
        )
    };
    if installing {
        match fingerprints(host) {
            Ok(current) => message.push_str(&current),
            Err(_) => {
                message.push_str("導入済みですが、導入後のSSHホスト鍵の指紋取得に失敗しました。現在の指紋は管理コンソールで確認してください。\n以下は事前確認時の指紋です。\n");
                message.push_str(&checked_fingerprints);
            }
        }
    } else {
        message.push_str(&checked_fingerprints);
    }
    Ok(message)
}

pub fn run(args: impl IntoIterator<Item = String>) -> Result<()> {
    let Some(opts) = Options::parse(args)? else {
        print!("{HELP}");
        return Ok(());
    };
    #[cfg(target_os = "linux")]
    {
        print!("{}", execute(&mut linux::LinuxHost::new(), &opts)?);
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = opts;
        Err("対応するLinux VPS上で実行してください".into())
    }
}
