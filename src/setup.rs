//! 対話式セットアップ。秘密値は端末から読み、コマンド引数には受け取らない。
use crate::error::{AppError, Result};
use std::path::PathBuf;

fn fail(code: &str) -> AppError {
    let message = match code {
        "SETTINGS_EXISTS" => {
            "同名アカウントまたは共有中継設定が存在します。既存設定は上書きしません。"
        }
        "SETTINGS_CHANGED" => "入力中に設定が変更されました。設定を確認して再実行してください。",
        "SETUP_BUSY" => "同じ保存先のセットアップが実行中です。完了後に再実行してください。",
        "KEY_EXISTS" => "専用SSH鍵が既に存在します。上書きしません。",
        "ACL_UNSAFE" => "保存先は本人所有で、他の利用者がアクセスできない権限にしてください。",
        "DEFAULT_ACCOUNT_INVALID" => {
            "既定アカウントには登録済みのアカウント設定名を指定してください。"
        }
        "PATH_INVALID" => {
            "プラグイン・Codex設定フォルダーの外に、リンクを含まない絶対パスを指定してください。"
        }
        "SSH_KEYGEN_REQUIRED" => "ssh-keygenで専用のパスフレーズなしSSH鍵を準備してください。",
        "INPUT_REQUIRED" => "必須項目を入力してください。",
        "MODE_INVALID" => "セットアップのオプションと組合せを確認してください。",
        _ => {
            "セットアップを完了できませんでした。入力形式・SSH鍵・設定・保存先の権限を確認してください。秘密値は表示しません。"
        }
    };
    AppError::new(code, message)
}

#[derive(Default)]
struct Options {
    prepare: bool,
    check: bool,
    settings: Option<PathBuf>,
    key: Option<PathBuf>,
    account: String,
    default: Option<String>,
    set_default: Option<String>,
}
impl Options {
    fn parse(args: &[String]) -> Result<Self> {
        let mut o = Self {
            account: "main".into(),
            ..Self::default()
        };
        let mut i = 0;
        while i < args.len() {
            let flag = args[i].as_str();
            match flag {
                "--setup" => {}
                "--prepare-key" => o.prepare = true,
                "--check" => o.check = true,
                "--settings-path"
                | "--private-key-path"
                | "--account"
                | "--default-account"
                | "--set-default-account" => {
                    i += 1;
                    let value = args
                        .get(i)
                        .filter(|v| !v.is_empty() && !v.starts_with("--"))
                        .ok_or_else(|| fail("MODE_INVALID"))?
                        .clone();
                    match flag {
                        "--settings-path" => o.settings = Some(value.into()),
                        "--private-key-path" => o.key = Some(value.into()),
                        "--account" => o.account = value,
                        "--default-account" => o.default = Some(value),
                        _ => o.set_default = Some(value),
                    }
                }
                _ => return Err(fail("MODE_INVALID")),
            }
            i += 1;
        }
        if (o.prepare && o.check)
            || (o.set_default.is_some() && (o.prepare || o.check || o.default.is_some()))
            || (o.default.is_some() && (o.prepare || o.check))
        {
            return Err(fail("MODE_INVALID"));
        }
        if !regex::Regex::new(r"\A[A-Za-z0-9_-]{1,64}\z")
            .unwrap()
            .is_match(&o.account)
            || o.account.eq_ignore_ascii_case("default_account")
        {
            return Err(fail("ACCOUNT_INVALID"));
        }
        Ok(o)
    }
}

pub async fn run(args: &[String]) -> Result<()> {
    let options = Options::parse(args)?;
    #[cfg(target_os = "macos")]
    {
        native::run(options).await
    }
    #[cfg(windows)]
    {
        let _ = options;
        let executable = std::env::current_exe().map_err(|_| fail("SETUP_UNAVAILABLE"))?;
        let script = executable
            .ancestors()
            .map(|p| p.join("setup/Setup-Plumivox.ps1"))
            .find(|p| p.is_file())
            .ok_or_else(|| fail("SETUP_UNAVAILABLE"))?;
        let mut command = tokio::process::Command::new("pwsh");
        command
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(script);
        let mut i = 0;
        while i < args.len() {
            let mapped = match args[i].as_str() {
                "--setup" => {
                    i += 1;
                    continue;
                }
                "--prepare-key" => "-PrepareKey",
                "--check" => "-Check",
                "--settings-path" => "-SettingsPath",
                "--private-key-path" => "-PrivateKeyPath",
                "--account" => "-Account",
                "--default-account" => "-DefaultAccount",
                "--set-default-account" => "-SetDefaultAccount",
                _ => return Err(fail("MODE_INVALID")),
            };
            command.arg(mapped);
            if !matches!(args[i].as_str(), "--prepare-key" | "--check") {
                i += 1;
                command.arg(&args[i]);
            }
            i += 1;
        }
        if command
            .status()
            .await
            .map_err(|_| fail("SETUP_UNAVAILABLE"))?
            .success()
        {
            Ok(())
        } else {
            Err(fail("SETUP_FAILED"))
        }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = options;
        Err(fail("SETUP_UNAVAILABLE"))
    }
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use fs2::FileExt;
    use serde_json::{Value, json};
    use std::{
        fs::{self, File, OpenOptions},
        io::{self, Write},
        os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        path::{Component, Path},
    };

    fn path_ok(path: &Path) -> Result<()> {
        if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(fail("PATH_INVALID"));
        }
        for ancestor in path.ancestors() {
            match fs::symlink_metadata(ancestor) {
                Ok(meta) if meta.file_type().is_symlink() => return Err(fail("PATH_INVALID")),
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(_) => return Err(fail("PATH_INVALID")),
            }
        }
        let home = dirs::home_dir().ok_or_else(|| fail("PATH_INVALID"))?;
        let mut blocked = vec![
            home.join(".codex"),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        ];
        if let Some(p) = std::env::var_os("CODEX_HOME") {
            blocked.push(p.into());
        }
        if let Ok(exe) = std::env::current_exe() {
            for ancestor in exe.ancestors().skip(1) {
                if ancestor.join(".codex-plugin/plugin.json").exists()
                    || ancestor.join("plugin.json").exists()
                {
                    blocked.push(ancestor.into());
                    break;
                }
            }
        }
        if blocked.iter().any(|root| path.starts_with(root)) {
            return Err(fail("PATH_INVALID"));
        }
        Ok(())
    }
    fn private(path: &Path, directory: bool) -> Result<()> {
        path_ok(path)?;
        let meta = fs::symlink_metadata(path).map_err(|_| fail("ACL_UNSAFE"))?;
        if meta.uid() != unsafe { libc::geteuid() }
            || meta.mode() & 0o077 != 0
            || meta.is_dir() != directory
            || (!directory && !meta.is_file())
        {
            return Err(fail("ACL_UNSAFE"));
        }
        Ok(())
    }
    fn directory(path: &Path) -> Result<()> {
        path_ok(path)?;
        if path.exists() {
            return private(path, true);
        }
        let parent = path.parent().ok_or_else(|| fail("PATH_INVALID"))?;
        if !parent.exists() {
            directory(parent)?;
        }
        // 新規作成分だけを制限し、既存の親フォルダーの権限は変更しない。
        std::os::unix::fs::DirBuilderExt::mode(&mut fs::DirBuilder::new(), 0o700)
            .create(path)
            .map_err(|_| fail("ACL_UNSAFE"))?;
        private(path, true)
    }
    fn snapshot(path: &Path) -> Result<Option<Vec<u8>>> {
        path_ok(path)?;
        match fs::symlink_metadata(path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(fail("SETTINGS_INVALID")),
            Ok(meta) => {
                if !meta.is_file() || meta.len() > 2097152 {
                    return Err(fail("SETTINGS_INVALID"));
                }
                private(path, false)?;
                let bytes = fs::read(path).map_err(|_| fail("SETTINGS_INVALID"))?;
                if bytes.len() > 2097152 {
                    return Err(fail("SETTINGS_INVALID"));
                }
                Ok(Some(bytes))
            }
        }
    }
    fn lock(path: &Path) -> Result<File> {
        let parent = path.parent().ok_or_else(|| fail("PATH_INVALID"))?;
        directory(parent)?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| fail("PATH_INVALID"))?;
        let lock_path = parent.join(format!(".{name}.plumivox-setup.lock"));
        path_ok(&lock_path)?;
        if lock_path.exists() {
            private(&lock_path, false)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&lock_path)
            .map_err(|_| fail("ACL_UNSAFE"))?;
        private(&lock_path, false)?;
        file.try_lock_exclusive().map_err(|_| fail("SETUP_BUSY"))?;
        Ok(file)
    }
    struct Staging(PathBuf);
    impl Drop for Staging {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    fn save(path: &Path, original: &Option<Vec<u8>>, doc: &Value) -> Result<()> {
        let parent = path.parent().ok_or_else(|| fail("PATH_INVALID"))?;
        private(parent, true)?;
        let staging =
            Staging(parent.join(format!(".plumivox-{:032x}.tmp", rand::random::<u128>())));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&staging.0)
            .map_err(|_| fail("SETTINGS_INVALID"))?;
        let bytes = serde_json::to_vec_pretty(doc).map_err(|_| fail("SETTINGS_INVALID"))?;
        if bytes.len() > 2097152 {
            return Err(fail("SETTINGS_INVALID"));
        }
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| fail("SETTINGS_INVALID"))?;
        private(&staging.0, false)?;
        if snapshot(path)? != *original {
            return Err(fail("SETTINGS_CHANGED"));
        }
        fs::rename(&staging.0, path).map_err(|_| fail("SETTINGS_INVALID"))?;
        File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|_| fail("SETTINGS_INVALID"))?;
        Ok(())
    }
    fn ask(prompt: &str, default: &str) -> Result<String> {
        print!(
            "{prompt}{}: ",
            if default.is_empty() {
                String::new()
            } else {
                format!(" [{default}]")
            }
        );
        io::stdout().flush().map_err(|_| fail("INPUT_REQUIRED"))?;
        let mut value = String::new();
        if io::stdin()
            .read_line(&mut value)
            .map_err(|_| fail("INPUT_REQUIRED"))?
            == 0
        {
            return Err(fail("INPUT_REQUIRED"));
        }
        let value = value.trim_end_matches(['\r', '\n']);
        Ok(if value.trim().is_empty() {
            default.into()
        } else {
            value.into()
        })
    }
    fn secret(prompt: &str, optional: bool) -> Result<String> {
        let value = rpassword::prompt_password(format!("{prompt}: "))
            .map_err(|_| fail("INPUT_REQUIRED"))?;
        if !optional && value.trim().is_empty() {
            return Err(fail("INPUT_REQUIRED"));
        }
        Ok(value)
    }
    fn port(prompt: &str, default: u16, min: u16) -> Result<u16> {
        ask(prompt, &default.to_string())?
            .parse::<u16>()
            .ok()
            .filter(|p| *p >= min)
            .ok_or_else(|| fail("PORT_INVALID"))
    }
    fn valid(pattern: &str, value: &str) -> bool {
        regex::Regex::new(pattern).unwrap().is_match(value)
    }
    async fn keygen(args: &[&str]) -> Result<()> {
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            tokio::process::Command::new("ssh-keygen")
                .args(args)
                .kill_on_drop(true)
                .output(),
        )
        .await
        .map_err(|_| fail("SSH_KEYGEN_REQUIRED"))?
        .map_err(|_| fail("SSH_KEYGEN_REQUIRED"))?;
        if !result.status.success() {
            return Err(fail("SSH_KEYGEN_REQUIRED"));
        }
        Ok(())
    }
    async fn validate_key(key: &Path) -> Result<()> {
        private(key.parent().ok_or_else(|| fail("PATH_INVALID"))?, true)?;
        private(key, false)?;
        keygen(&[
            "-y",
            "-P",
            "",
            "-f",
            key.to_str().ok_or_else(|| fail("PATH_INVALID"))?,
        ])
        .await
    }
    async fn prepare_key(key: &Path) -> Result<()> {
        path_ok(key)?;
        let public = PathBuf::from(format!(
            "{}.pub",
            key.to_str().ok_or_else(|| fail("PATH_INVALID"))?
        ));
        path_ok(&public)?;
        let _lock = lock(key)?;
        if key.exists() || public.exists() {
            return Err(fail("KEY_EXISTS"));
        }
        keygen(&[
            "-q",
            "-t",
            "ed25519",
            "-N",
            "",
            "-C",
            "plumivox",
            "-f",
            key.to_str().unwrap(),
        ])
        .await?;
        private(key, false)?;
        // ssh-keygenの公開鍵は秘密を含まないが、保存先と合わせて制限する。
        let metadata = fs::symlink_metadata(&public).map_err(|_| fail("SSH_KEYGEN_REQUIRED"))?;
        if metadata.uid() != unsafe { libc::geteuid() } || !metadata.is_file() {
            return Err(fail("ACL_UNSAFE"));
        }
        fs::set_permissions(&public, fs::Permissions::from_mode(0o600))
            .map_err(|_| fail("ACL_UNSAFE"))?;
        private(&public, false)?;
        println!(
            "VPS側に登録する公開鍵:\n{}",
            fs::read_to_string(public)
                .map_err(|_| fail("SSH_KEYGEN_REQUIRED"))?
                .trim()
        );
        Ok(())
    }
    fn names(doc: &Value) -> Vec<String> {
        doc["twitterapi_io"]
            .as_object()
            .map(|m| {
                m.iter()
                    .filter(|(k, v)| k.as_str() != "default_account" && v.is_object())
                    .map(|(k, _)| k.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub async fn run(o: Options) -> Result<()> {
        let settings = crate::config::resolve_secrets_path(o.settings);
        let home = dirs::home_dir().ok_or_else(|| fail("PATH_INVALID"))?;
        let key = o.key.unwrap_or_else(|| {
            home.join("Library/Application Support/Kagayoi/Plumivox/keys/plumivox_ed25519")
        });
        if o.prepare {
            return prepare_key(&key).await;
        }
        path_ok(&settings)?;
        let _lock = lock(&settings)?;
        let original = snapshot(&settings)?;
        let mut doc: Value = match &original {
            Some(b) => serde_json::from_slice(b).map_err(|_| fail("SETTINGS_INVALID"))?,
            None => json!({}),
        };
        if !doc.is_object() || doc.get("twitterapi_io").is_some_and(|v| !v.is_object()) {
            return Err(fail("SETTINGS_INVALID"));
        }
        let mut accounts = names(&doc);
        let existing_default = doc["twitterapi_io"]["default_account"]
            .as_str()
            .filter(|n| accounts.iter().any(|a| a == n))
            .map(str::to_owned);
        if doc["twitterapi_io"].get("default_account").is_some()
            && existing_default.is_none()
            && o.set_default.is_none()
            && o.default.is_none()
        {
            return Err(fail("DEFAULT_ACCOUNT_INVALID"));
        }
        if o.check {
            let store = crate::config::AccountStore::new(Some(settings), None, None);
            store.get_login_credentials(Some(&o.account)).await?;
            if store
                .get_proxy_configuration(Some(&o.account), None)
                .await?
                .is_null()
            {
                return Err(fail("SETTINGS_INVALID"));
            }
            let key = doc["plumivox"]["managed_proxy"]["private_key_file"]
                .as_str()
                .ok_or_else(|| fail("SETTINGS_INVALID"))?;
            validate_key(Path::new(key)).await?;
            println!(
                "設定形式・専用SSH鍵・所有者・権限を確認しました。ネットワーク通信は実行していません。"
            );
            return Ok(());
        }
        if let Some(default) = o.set_default {
            if !accounts.contains(&default) {
                return Err(fail("DEFAULT_ACCOUNT_INVALID"));
            }
            doc["twitterapi_io"]["default_account"] = json!(default);
        } else {
            if doc["twitterapi_io"].get(&o.account).is_some()
                || doc["plumivox"].get("managed_proxy").is_some()
            {
                return Err(fail("SETTINGS_EXISTS"));
            }
            if doc.get("plumivox").is_some_and(|v| !v.is_object()) {
                return Err(fail("SETTINGS_INVALID"));
            }
            accounts.push(o.account.clone());
            let default = match o.default.or(existing_default) {
                Some(d) => d,
                None if accounts.len() == 1 => o.account.clone(),
                None => {
                    println!("アカウント設定名: {}", accounts.join(", "));
                    ask("既定にするアカウント設定名", "")?
                }
            };
            if !accounts.contains(&default) {
                return Err(fail("DEFAULT_ACCOUNT_INVALID"));
            }
            let user = ask("Xユーザー名 (@なし)", "")?;
            if !valid(r"\A[A-Za-z0-9_]{1,15}\z", &user) {
                return Err(fail("USER_INVALID"));
            }
            let host = ask("VPSホスト名/IP", "")?.to_ascii_lowercase();
            let ssh_user = ask("SSHユーザー", "plumivox")?;
            let ssh_port = port("SSHポート", 22, 1)?;
            let local_port = port("PC内部ポート", 43128, 1024)?;
            let remote_port = port("VPS内部ポート", 43129, 1024)?;
            println!(
                "VPSコンソールで ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub -E sha256 を実行し指紋を照合してください。"
            );
            let fingerprint = ask("コンソールで確認済みのSHA256指紋", "")?;
            if !valid(r"\A[A-Za-z0-9._:\[\]-]{1,253}\z", &host)
                || !valid(r"\A[A-Za-z0-9._-]{1,64}\z", &ssh_user)
                || !valid(r"\ASHA256:[A-Za-z0-9+/]{43}\z", &fingerprint)
            {
                return Err(fail("SETTINGS_INVALID"));
            }
            validate_key(&key).await?;
            let public_port = port("公開プロキシポート", 3128, 1024)?;
            let proxy_user = ask("プロキシユーザー", "plumivox")?;
            if !valid(r"\A[A-Za-z0-9_-]{1,64}\z", &proxy_user) {
                return Err(fail("PROXY_USER_INVALID"));
            }
            let mut profile = json!({"user_name":user,"api_key":secret("twitterapi.io APIキー",false)?,
                "email":secret("Xログインメール",false)?,"password":secret("Xパスワード",false)?,"proxy_mode":"managed"});
            let totp = secret("Base32の2FA秘密鍵 (未使用なら空欄)", true)?;
            if !totp.is_empty() {
                if !valid(r"\A[A-Za-z2-7]+={0,6}\z", &totp) || valid(r"\A[0-9]{6}\z", &totp) {
                    return Err(fail("TOTP_INVALID"));
                }
                profile["totp_secret"] = json!(totp);
            }
            let proxy_password = secret("プロキシパスワード", false)?;
            let mut proxy = url::Url::parse(&format!("http://{host}:{public_port}"))
                .map_err(|_| fail("PROXY_INVALID"))?;
            proxy
                .set_username(&proxy_user)
                .map_err(|_| fail("PROXY_INVALID"))?;
            proxy
                .set_password(Some(&proxy_password))
                .map_err(|_| fail("PROXY_INVALID"))?;
            profile["proxy"] = json!(proxy.as_str());
            if doc.get("twitterapi_io").is_none() {
                doc["twitterapi_io"] = json!({});
            }
            if doc.get("plumivox").is_none() {
                doc["plumivox"] = json!({});
            }
            doc["twitterapi_io"][&o.account] = profile;
            doc["twitterapi_io"]["default_account"] = json!(default);
            doc["plumivox"]["managed_proxy"] = json!({"ssh_host":host,"ssh_user":ssh_user,"ssh_port":ssh_port,
                "local_port":local_port,"remote_port":remote_port,"private_key_file":key,"host_key_sha256":fingerprint});
        }
        save(&settings, &original, &doc)?;
        println!("設定を保存しました。秘密値は表示していません。");
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        struct Area(PathBuf);
        impl Area {
            fn new() -> Self {
                let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
                    "plumivox-setup-test-{:032x}",
                    rand::random::<u128>()
                ));
                directory(&root).unwrap();
                Self(root)
            }
        }
        impl Drop for Area {
            fn drop(&mut self) {
                // このテストだけが作成した専用ディレクトリを回収する。
                if self
                    .0
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("plumivox-setup-test-")
                {
                    let _ = fs::remove_dir_all(&self.0);
                }
            }
        }

        #[test]
        fn settings_changed_during_input_are_preserved() {
            let area = Area::new();
            let path = area.0.join("settings.json");
            save(&path, &None, &json!({"original":true})).unwrap();
            let before = snapshot(&path).unwrap();
            let changed = b"{\"changed_by_other_process\":true}";
            fs::write(&path, changed).unwrap();
            assert_eq!(
                save(&path, &before, &json!({"replacement":true}))
                    .unwrap_err()
                    .code,
                "SETTINGS_CHANGED"
            );
            assert_eq!(fs::read(path).unwrap(), changed);
            assert_eq!(fs::read_dir(&area.0).unwrap().count(), 1);
        }

        #[test]
        fn setup_lock_excludes_second_writer() {
            let area = Area::new();
            let path = area.0.join("settings.json");
            let first = lock(&path).unwrap();
            assert_eq!(lock(&path).unwrap_err().code, "SETUP_BUSY");
            drop(first);
            assert!(lock(&path).is_ok());
        }

        #[test]
        fn setup_rejects_symbolic_links_and_shared_permissions() {
            let area = Area::new();
            let target = area.0.join("settings.json");
            save(&target, &None, &json!({})).unwrap();
            let link = area.0.join("linked.json");
            std::os::unix::fs::symlink(&target, &link).unwrap();
            assert_eq!(snapshot(&link).unwrap_err().code, "PATH_INVALID");
            fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
            assert_eq!(snapshot(&target).unwrap_err().code, "ACL_UNSAFE");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conflicting_setup_modes_and_secret_arguments_are_rejected() {
        for args in [
            vec!["--prepare-key", "--check"],
            vec!["--set-default-account", "main", "--default-account", "main"],
            vec!["--api-key", "secret"],
        ] {
            assert!(
                Options::parse(&args.into_iter().map(str::to_owned).collect::<Vec<_>>()).is_err()
            );
        }
        let options = Options::parse(&["--set-default-account".into(), "main".into()]).unwrap();
        assert_eq!(options.set_default.as_deref(), Some("main"));
    }
}
