//! 対話式セットアップ。秘密値は端末から読み、コマンド引数には受け取らない。
use crate::error::{AppError, Result};
use crate::locale::tr;
use std::path::PathBuf;

/// セットアップの失敗。端末の利用者が読むので、文言は利用者の言語の辞書から引く（コードは英字のまま）。
/// 整合テストがキーを拾えるよう、キーは match の各腕に文字列リテラルで書く。
fn fail(code: &str) -> AppError {
    let message = match code {
        "SETTINGS_EXISTS" => tr("setup.error.settings_exists"),
        "SETTINGS_CHANGED" => tr("setup.error.settings_changed"),
        "SETTINGS_SAVE_UNCERTAIN" => tr("setup.error.settings_save_uncertain"),
        "SETTINGS_TEMP_RETAINED" => tr("setup.error.settings_temp_retained"),
        "SETUP_BUSY" => tr("setup.error.setup_busy"),
        "KEY_EXISTS" => tr("setup.error.key_exists"),
        "KEY_GENERATION_INCOMPLETE" => tr("setup.error.key_generation_incomplete"),
        "ACL_UNSAFE" => tr("setup.error.acl_unsafe"),
        "DEFAULT_ACCOUNT_INVALID" => tr("setup.error.default_account_invalid"),
        "PATH_INVALID" => tr("setup.error.path_invalid"),
        "SSH_KEYGEN_REQUIRED" => tr("setup.error.ssh_keygen_required"),
        "INPUT_REQUIRED" => tr("setup.error.input_required"),
        "MODE_INVALID" => tr("setup.error.mode_invalid"),
        _ => tr("setup.error.generic"),
    };
    AppError::new(code, message)
}

/// `--setup --check` で設定の読込み（`config.rs`）から返ったエラーを、利用者の言語の文言に置き換える。
/// `config.rs` の文言は MCP 応答として AI が読むため英語のままにしてあり、端末に出すここでだけ訳す。
/// 辞書に対応の無いコードは元のエラー（英語）をそのまま返す。
#[cfg(any(windows, target_os = "macos"))]
fn localize_check_error(error: AppError) -> AppError {
    let message = match error.code.as_str() {
        "ACCOUNT_INVALID" => tr("setup.check.account_invalid"),
        "CONFIG_UNAVAILABLE" => tr("setup.check.config_unavailable"),
        "ACCOUNT_DEFAULT_INVALID" => tr("setup.check.account_default_invalid"),
        "ACCOUNT_REQUIRED" => tr("setup.check.account_required"),
        "ACCOUNT_UNKNOWN" => tr("setup.check.account_unknown"),
        "LOGIN_CREDENTIALS_INVALID" => tr("setup.check.login_credentials_invalid"),
        "LOGIN_UNAVAILABLE" => tr("setup.check.login_unavailable"),
        "PROXY_CONFIG" => tr("setup.check.proxy_config"),
        _ => return error,
    };
    AppError {
        message: message.to_owned(),
        ..error
    }
}

/// アカウント設定名とプロキシ利用者名の書式（セットアップだけが使う）。
static NAME: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"\A[A-Za-z0-9_-]{1,64}\z").unwrap());

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
        if !NAME.is_match(&o.account) || o.account.eq_ignore_ascii_case("default_account") {
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
    use crate::locale::tr_with;
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
        // Codex・Claude Code などホストの設定フォルダー、リポジトリ、プラグインルートを拒否する。
        let mut blocked = crate::agent_dirs::agent_user_roots(&home);
        blocked.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        blocked.extend(crate::agent_dirs::executable_plugin_root());
        // Setup（Velopack）の導入先（.app）は更新・削除で入れ替わるため、保存先にしない。
        blocked.extend(crate::agent_dirs::velopack_install_roots());
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
    async fn keygen(args: &[&str]) -> Result<()> {
        // 呼び出し側の引数は `-N ""`（パスフレーズ空）・`-P ""`・`-f` 指定で、入力を求めない非対話の形。
        // 念のため stdin も閉じ、Windows ではコンソール窓を出さない（CREATE_NO_WINDOW）。
        let mut command = tokio::process::Command::new(keygen_executable());
        command
            .args(args)
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x0800_0000);
        let result = tokio::time::timeout(std::time::Duration::from_secs(30), command.output())
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
            let public_key =
                fs::read_to_string(&public).map_err(|_| fail("SSH_KEYGEN_REQUIRED"))?;
            println!(
                "{}",
                tr_with("setup.public_key", &[("key", public_key.trim())])
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
            store
                .get_login_credentials(Some(&o.account))
                .await
                .map_err(localize_check_error)?;
            if store
                .get_proxy_configuration(Some(&o.account), None)
                .await
                .map_err(localize_check_error)?
                .is_null()
            {
                return Err(fail("SETTINGS_INVALID"));
            }
            let key = doc["plumivox"]["managed_proxy"]["private_key_file"]
                .as_str()
                .ok_or_else(|| fail("SETTINGS_INVALID"))?;
            validate_key(Path::new(key)).await?;
            println!("{}", tr("setup.check_ok"));
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
                    println!(
                        "{}",
                        tr_with("setup.account_names", &[("names", &accounts.join(", "))])
                    );
                    ask(tr("setup.prompt.default_account"), "")?
                }
            };
            if !accounts.contains(&default) {
                return Err(fail("DEFAULT_ACCOUNT_INVALID"));
            }
            let user = ask(tr("setup.prompt.x_user"), "")?;
            if !crate::config::USER_NAME.is_match(&user) {
                return Err(fail("USER_INVALID"));
            }
            let host = ask(tr("setup.prompt.vps_host"), "")?.to_ascii_lowercase();
            let ssh_user = ask(tr("setup.prompt.ssh_user"), "plumivox")?;
            let ssh_port = port(tr("setup.prompt.ssh_port"), 22, 1)?;
            let local_port = port(tr("setup.prompt.local_port"), 43128, 1024)?;
            let remote_port = port(tr("setup.prompt.remote_port"), 43129, 1024)?;
            println!("{}", tr("setup.fingerprint_hint"));
            let fingerprint = ask(tr("setup.prompt.fingerprint"), "")?;
            if !crate::config::SSH_HOST.is_match(&host)
                || !crate::config::SSH_USER.is_match(&ssh_user)
                || !crate::config::HOST_KEY.is_match(&fingerprint)
            {
                return Err(fail("SETTINGS_INVALID"));
            }
            validate_key(&key).await?;
            let public_port = port(tr("setup.prompt.public_port"), 3128, 1024)?;
            let proxy_user = ask(tr("setup.prompt.proxy_user"), "plumivox")?;
            if !super::NAME.is_match(&proxy_user) {
                return Err(fail("PROXY_USER_INVALID"));
            }
            let mut profile = json!({"user_name":user,"api_key":secret(tr("setup.prompt.api_key"),false)?,
                "email":secret(tr("setup.prompt.email"),false)?,"password":secret(tr("setup.prompt.password"),false)?,"proxy_mode":"managed"});
            let totp = secret(tr("setup.prompt.totp"), true)?;
            if !totp.is_empty() {
                if !crate::config::TOTP_SECRET.is_match(&totp)
                    || crate::config::TOTP_CODE.is_match(&totp)
                {
                    return Err(fail("TOTP_INVALID"));
                }
                profile["totp_secret"] = json!(totp);
            }
            let proxy_password = secret(tr("setup.prompt.proxy_password"), false)?;
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
        println!("{}", tr("setup.saved"));
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
