//! 対話式セットアップ。秘密値は端末から読み、コマンド引数には受け取らない。
use crate::error::{AppError, Result};
use std::path::PathBuf;

fn fail(code: &str) -> AppError {
    let message = match code {
        "SETTINGS_EXISTS" => {
            "同名アカウントまたは共有中継設定が存在します。既存設定は上書きしません。"
        }
        "SETTINGS_CHANGED" => "入力中に設定が変更されました。設定を確認して再実行してください。",
        "SETTINGS_SAVE_UNCERTAIN" => {
            "設定の保存結果を確認できませんでした。正本と同じフォルダーの.plumivox-*.tmpを確認し、今回の未使用ファイルと本人が確認できたものだけを退避してから再実行してください。"
        }
        "SETTINGS_TEMP_RETAINED" => {
            "今回の一時設定ファイルを回収できませんでした。正本と同じフォルダーの.plumivox-*.tmpを確認し、今回の未使用ファイルと本人が確認できたものだけを退避してから再実行してください。"
        }
        "SETUP_BUSY" => "同じ保存先のセットアップが実行中です。完了後に再実行してください。",
        "KEY_EXISTS" => "専用SSH鍵が既に存在します。上書きしません。",
        "KEY_GENERATION_INCOMPLETE" => {
            "鍵の生成先にファイルが残った状態で処理に失敗しました。指定した秘密鍵と.pubを確認し、本人が今回の未使用・未完了ファイルと確認できたものだけを退避してから再実行してください。自動削除・上書きはしません。"
        }
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
    #[cfg(any(windows, target_os = "macos"))]
    {
        native::run(options).await
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = options;
        Err(fail("SETUP_UNAVAILABLE"))
    }
}

#[cfg(windows)]
#[path = "setup_windows.rs"]
mod windows;

#[cfg(any(windows, target_os = "macos"))]
mod native {
    #[cfg(windows)]
    use super::windows::{lock, path_ok, private};
    use super::*;
    #[cfg(target_os = "macos")]
    use fs2::FileExt;
    use serde_json::{Value, json};
    #[cfg(target_os = "macos")]
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
    #[cfg(target_os = "macos")]
    use std::path::Component;
    use std::{
        fs,
        io::{self, Write},
        path::Path,
    };
    #[cfg(target_os = "macos")]
    use std::{
        fs::{File, OpenOptions},
        io::Read,
    };

    #[cfg(target_os = "macos")]
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
    #[cfg(target_os = "macos")]
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
    #[cfg(target_os = "macos")]
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
        read_snapshot(path, true)
    }
    #[cfg(windows)]
    fn writable_snapshot(path: &Path) -> Result<Option<Vec<u8>>> {
        read_snapshot(path, false)
    }
    fn read_snapshot(path: &Path, private_required: bool) -> Result<Option<Vec<u8>>> {
        #[cfg(windows)]
        {
            return super::windows::snapshot(path, private_required);
        }
        #[cfg(target_os = "macos")]
        {
            path_ok(path)?;
            match fs::symlink_metadata(path) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(_) => Err(fail("SETTINGS_INVALID")),
                Ok(meta) => {
                    if !meta.is_file() || meta.len() > 2097152 {
                        return Err(fail("SETTINGS_INVALID"));
                    }
                    let _ = private_required;
                    private(path, false)?;
                    let file = File::open(path).map_err(|_| fail("SETTINGS_INVALID"))?;
                    if !file
                        .metadata()
                        .map_err(|_| fail("SETTINGS_INVALID"))?
                        .is_file()
                    {
                        return Err(fail("SETTINGS_INVALID"));
                    }
                    let mut bytes = Vec::new();
                    file.take(2097153)
                        .read_to_end(&mut bytes)
                        .map_err(|_| fail("SETTINGS_INVALID"))?;
                    if bytes.len() > 2097152 {
                        return Err(fail("SETTINGS_INVALID"));
                    }
                    Ok(Some(bytes))
                }
            }
        }
    }
    #[cfg(target_os = "macos")]
    fn lock(path: &Path) -> Result<File> {
        let parent = path.parent().ok_or_else(|| fail("PATH_INVALID"))?;
        directory(parent)?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| fail("PATH_INVALID"))?;
        let lock_path = parent.join(format!(".{name}.plumivox-setup.lock"));
        path_ok(&lock_path)?;
        let exists = lock_path.exists();
        if exists {
            private(&lock_path, false)?;
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(target_os = "macos")]
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        let file = options.open(&lock_path).map_err(|_| fail("ACL_UNSAFE"))?;
        private(&lock_path, false)?;
        file.try_lock_exclusive().map_err(|_| fail("SETUP_BUSY"))?;
        Ok(file)
    }
    #[cfg(target_os = "macos")]
    struct Staging(PathBuf);
    #[cfg(target_os = "macos")]
    impl Drop for Staging {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    fn save(path: &Path, original: &Option<Vec<u8>>, doc: &Value) -> Result<()> {
        let parent = path.parent().ok_or_else(|| fail("PATH_INVALID"))?;
        #[cfg(windows)]
        let _parents = super::windows::guard_parent(path, true)?;
        #[cfg(target_os = "macos")]
        private(parent, true)?;
        let staging_path = parent.join(format!(".plumivox-{:032x}.tmp", rand::random::<u128>()));
        #[cfg(target_os = "macos")]
        let _staging = Staging(staging_path.clone());
        #[cfg(target_os = "macos")]
        let mut options = OpenOptions::new();
        #[cfg(target_os = "macos")]
        options.write(true).create_new(true);
        #[cfg(target_os = "macos")]
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        #[cfg(target_os = "macos")]
        let mut file = options
            .open(&staging_path)
            .map_err(|_| fail("SETTINGS_INVALID"))?;
        #[cfg(windows)]
        let mut file = super::windows::staging(&staging_path)?;
        let result = (|| {
            let bytes = serde_json::to_vec_pretty(doc).map_err(|_| fail("SETTINGS_INVALID"))?;
            if bytes.len() > 2097152 {
                return Err(fail("SETTINGS_INVALID"));
            }
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|_| fail("SETTINGS_INVALID"))?;
            #[cfg(target_os = "macos")]
            private(&staging_path, false)?;
            #[cfg(windows)]
            super::windows::verify_staging(&file)?;
            #[cfg(windows)]
            let current = writable_snapshot(path)?;
            #[cfg(target_os = "macos")]
            let current = snapshot(path)?;
            if current != *original {
                return Err(fail("SETTINGS_CHANGED"));
            }
            #[cfg(windows)]
            super::windows::publish(&mut file, path, original.is_some())?;
            #[cfg(target_os = "macos")]
            {
                drop(file);
                replace(&staging_path, path, original.is_some())?;
            }
            #[cfg(target_os = "macos")]
            File::open(parent)
                .and_then(|f| f.sync_all())
                .map_err(|_| fail("SETTINGS_INVALID"))?;
            Ok(())
        })();
        #[cfg(windows)]
        {
            file.finish(result)
        }
        #[cfg(target_os = "macos")]
        {
            result
        }
    }
    #[cfg(target_os = "macos")]
    fn replace(staging: &Path, destination: &Path, replace_existing: bool) -> Result<()> {
        if replace_existing {
            fs::rename(staging, destination).map_err(|_| fail("SETTINGS_INVALID"))
        } else {
            use std::{ffi::CString, os::unix::ffi::OsStrExt};
            let source =
                CString::new(staging.as_os_str().as_bytes()).map_err(|_| fail("PATH_INVALID"))?;
            let destination = CString::new(destination.as_os_str().as_bytes())
                .map_err(|_| fail("PATH_INVALID"))?;
            // 宛先の初回作成と一時名の除去を一度で行い、既存ファイルを置換しない。
            // RENAME_EXCL未対応のファイルシステムでは安全に失敗する。
            if unsafe { libc::renamex_np(source.as_ptr(), destination.as_ptr(), libc::RENAME_EXCL) }
                == 0
            {
                Ok(())
            } else {
                let error = io::Error::last_os_error();
                Err(fail(if error.kind() == io::ErrorKind::AlreadyExists {
                    "SETTINGS_CHANGED"
                } else {
                    "SETTINGS_INVALID"
                }))
            }
        }
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
        #[cfg(windows)]
        let value = super::windows::secret(prompt)?;
        #[cfg(target_os = "macos")]
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
            tokio::process::Command::new(keygen_executable())
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
    fn keygen_executable() -> PathBuf {
        #[cfg(windows)]
        for (variable, suffix) in [
            ("WINDIR", "System32/OpenSSH/ssh-keygen.exe"),
            ("ProgramFiles", "Git/usr/bin/ssh-keygen.exe"),
        ] {
            if let Some(root) = std::env::var_os(variable) {
                let path = PathBuf::from(root).join(suffix);
                if path.is_file() {
                    return path;
                }
            }
        }
        PathBuf::from("ssh-keygen")
    }
    async fn validate_key(key: &Path) -> Result<()> {
        #[cfg(target_os = "macos")]
        private(key.parent().ok_or_else(|| fail("PATH_INVALID"))?, true)?;
        #[cfg(target_os = "macos")]
        private(key, false)?;
        #[cfg(windows)]
        let _parents = super::windows::guard_parent(key, false)?;
        #[cfg(windows)]
        let _key = super::windows::read_file(key, true)?;
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
        #[cfg(windows)]
        if let Some(parent) = key.parent().filter(|p| p.exists()) {
            private(parent, true)?;
        }
        let _lock = lock(key)?;
        #[cfg(windows)]
        let _parents = super::windows::guard_parent(key, true)?;
        if key.exists() || public.exists() {
            return Err(fail("KEY_EXISTS"));
        }
        let generated = async {
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
            #[cfg(windows)]
            super::windows::protect(key)?;
            private(key, false)?;
            // ssh-keygenの公開鍵は秘密を含まないが、保存先と合わせて制限する。
            #[cfg(target_os = "macos")]
            let metadata =
                fs::symlink_metadata(&public).map_err(|_| fail("SSH_KEYGEN_REQUIRED"))?;
            #[cfg(target_os = "macos")]
            if metadata.uid() != unsafe { libc::geteuid() } || !metadata.is_file() {
                return Err(fail("ACL_UNSAFE"));
            }
            #[cfg(target_os = "macos")]
            fs::set_permissions(&public, fs::Permissions::from_mode(0o600))
                .map_err(|_| fail("ACL_UNSAFE"))?;
            #[cfg(windows)]
            super::windows::protect(&public)?;
            private(&public, false)?;
            println!(
                "VPS側に登録する公開鍵:\n{}",
                fs::read_to_string(&public)
                    .map_err(|_| fail("SSH_KEYGEN_REQUIRED"))?
                    .trim()
            );
            Ok(())
        }
        .await;
        if generated.is_err()
            && (fs::symlink_metadata(key).is_ok() || fs::symlink_metadata(&public).is_ok())
        {
            return Err(fail("KEY_GENERATION_INCOMPLETE"));
        }
        generated
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
        #[cfg(target_os = "macos")]
        let home = dirs::home_dir().ok_or_else(|| fail("PATH_INVALID"))?;
        #[cfg(windows)]
        let key = o.key.unwrap_or_else(super::windows::default_key);
        #[cfg(target_os = "macos")]
        let key = o.key.unwrap_or_else(|| {
            home.join("Library/Application Support/Kagayoi/Plumivox/keys/plumivox_ed25519")
        });
        if o.prepare {
            return prepare_key(&key).await;
        }
        path_ok(&settings)?;
        let _lock = lock(&settings)?;
        #[cfg(windows)]
        let original = if o.check {
            snapshot(&settings)?
        } else {
            writable_snapshot(&settings)?
        };
        #[cfg(target_os = "macos")]
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

    #[cfg(all(test, target_os = "macos"))]
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
        fn initial_publish_preserves_concurrent_destination_and_publishes_when_absent() {
            let area = Area::new();
            let staging = area.0.join("staging.json");
            let destination = area.0.join("settings.json");
            fs::write(&staging, b"new settings").unwrap();
            fs::write(&destination, b"concurrent settings").unwrap();
            assert_eq!(
                replace(&staging, &destination, false).unwrap_err().code,
                "SETTINGS_CHANGED"
            );
            assert_eq!(fs::read(&destination).unwrap(), b"concurrent settings");
            assert_eq!(fs::read(&staging).unwrap(), b"new settings");
            let absent = area.0.join("absent.json");
            replace(&staging, &absent, false).unwrap();
            assert_eq!(fs::read(absent).unwrap(), b"new settings");
            assert!(!staging.exists());
        }

        #[test]
        fn snapshot_accepts_exact_byte_limit_and_rejects_one_extra_byte() {
            let area = Area::new();
            let path = area.0.join("settings.json");
            save(&path, &None, &json!({})).unwrap();
            let bytes = vec![b' '; 2097152];
            fs::write(&path, &bytes).unwrap();
            assert_eq!(snapshot(&path).unwrap().unwrap().len(), bytes.len());
            let mut file = OpenOptions::new().append(true).open(&path).unwrap();
            file.write_all(b" ").unwrap();
            drop(file);
            assert_eq!(snapshot(&path).unwrap_err().code, "SETTINGS_INVALID");
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
