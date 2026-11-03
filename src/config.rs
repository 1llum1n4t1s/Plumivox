use crate::error::{AppError, Result};
use crate::session::{SessionLease, SessionStorage, read_legacy_protected_cookie};
use regex::Regex;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, path::PathBuf};
use tokio_util::sync::CancellationToken;

const MAX_BYTES: usize = 2 * 1024 * 1024;

/// 起動時に正本を固定し、読込みに失敗しても別の設定へ切り替えない。
pub fn resolve_secrets_path(explicit: Option<PathBuf>) -> PathBuf {
    if let Some(path) = std::env::var_os("PLUMIVOX_SECRETS_FILE").filter(|s| !s.is_empty()) {
        return path.into();
    }
    if let Some(path) = explicit.filter(|p| !p.as_os_str().is_empty()) {
        return path;
    }
    // Nodeのos.homedir()と同じWindows環境変数の優先順位を維持する。
    // dirs::home_dir()単体はKnown Folderを返し、USERPROFILEを参照しない。
    let home = if cfg!(windows) {
        std::env::var_os("USERPROFILE")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .or_else(dirs::home_dir)
            .unwrap_or_default()
    } else {
        dirs::home_dir().unwrap_or_default()
    };
    let legacy = home.join("dev").join("Secret").join("secrets.json");
    if legacy.exists() {
        return legacy;
    }
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Kagayoi/Plumivox/settings.json")
    } else {
        std::env::var_os("LOCALAPPDATA")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData/Local"))
            .join("Kagayoi/Plumivox/settings.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_default_is_not_replaced_by_only_account() {
        // 明示名は不正な既定設定より優先する。
        let secrets = json!({"twitterapi_io":{"default_account":17,"first":{}}});
        let configs = AccountStore::profiles(&secrets);
        assert_eq!(
            AccountStore::resolve_name(&secrets, configs, Some("first".into())).unwrap(),
            "first"
        );
        assert_eq!(
            AccountStore::resolve_name(&secrets, configs, None)
                .unwrap_err()
                .code,
            "ACCOUNT_DEFAULT_INVALID"
        );
    }

    #[test]
    fn login_rejects_authentication_codes_as_totp_secrets() {
        for value in [json!("123456"), json!("111111"), json!(true), json!("A B")] {
            assert!(!valid_totp(&value));
        }
        for value in [
            Value::Null,
            json!(""),
            json!("   "),
            json!("JBSWY3DPEHPK3PXP"),
            json!("JBSWY3DP="),
        ] {
            assert!(valid_totp(&value));
        }
    }

    #[test]
    fn proxy_rejects_unsafe_or_unsupported_urls() {
        for value in [
            " http://host:8080",
            "http://host:8080 ",
            "http://host/#fragment",
            "socks5://host:8080",
            "http://ho\nst:8080",
        ] {
            assert!(!valid_proxy(&json!(value)), "{value:?}");
        }
        assert!(valid_proxy(&json!("http://user:password@host:8080")));
        assert!(valid_proxy(&json!("https://host:8443")));
    }
}

fn error(code: &str, message: &str) -> AppError {
    AppError::new(code, message)
}
fn nonempty(value: &Value) -> bool {
    value.as_str().is_some_and(|s| !s.trim().is_empty())
}
fn matches(pattern: &str, value: &Value) -> bool {
    value
        .as_str()
        .is_some_and(|s| Regex::new(pattern).unwrap().is_match(s))
}
fn valid_user(value: &Value) -> bool {
    matches(r"\A[A-Za-z0-9_]{1,15}\z", value)
}
fn valid_totp(value: &Value) -> bool {
    value.is_null()
        || value.as_str().is_some_and(|s| {
            s.trim().is_empty()
                || (Regex::new(r"\A[A-Za-z2-7]+={0,6}\z").unwrap().is_match(s)
                    && !Regex::new(r"\A[0-9]{6}\z").unwrap().is_match(s))
        })
}
fn valid_proxy(value: &Value) -> bool {
    let Some(s) = value.as_str() else {
        return false;
    };
    if s.is_empty() || s.trim() != s || s.chars().any(|c| c.is_whitespace() || c <= '\u{1f}') {
        return false;
    }
    url::Url::parse(s).is_ok_and(|u| {
        matches!(u.scheme(), "http" | "https") && u.host_str().is_some() && u.fragment().is_none()
    })
}
fn normalized(account: &str, config: &Value) -> Value {
    let mut result = json!({"account":account,
        "userName":if valid_user(&config["user_name"]) { config["user_name"].clone() } else { json!("") },
        "apiKey":if nonempty(&config["api_key"]) { config["api_key"].clone() } else { json!("") }});
    if valid_proxy(&config["proxy"]) {
        result["proxy"] = config["proxy"].clone();
    }
    result
}
fn read_ready(account: &Value) -> bool {
    nonempty(&account["userName"]) && nonempty(&account["apiKey"])
}
fn login_ready(config: &Value, account: &Value) -> bool {
    read_ready(account)
        && nonempty(&account["proxy"])
        && nonempty(&config["email"])
        && nonempty(&config["password"])
        && valid_totp(&config["totp_secret"])
}
fn hash(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}
fn invalid_account() -> AppError {
    error(
        "ACCOUNT_INVALID",
        "APIキーと有効なユーザー名を設定してください。",
    )
}
fn changed() -> AppError {
    error(
        "AUTH_CONFIG_CHANGED",
        "ログイン中にアカウント設定が変更されました。設定を確認して再実行してください。",
    )
}

#[derive(Clone)]
pub struct AccountStore {
    source: PathBuf,
    profile: Option<String>,
    pub sessions: SessionStorage,
    /// 既存のWindowsセッションを参照する場合だけ指定する。
    pub session_paths: HashMap<String, PathBuf>,
}

impl AccountStore {
    pub fn new(
        secrets_path: Option<PathBuf>,
        profile: Option<String>,
        state_directory: Option<PathBuf>,
    ) -> Self {
        Self {
            source: resolve_secrets_path(secrets_path),
            profile,
            sessions: SessionStorage::new(state_directory),
            session_paths: HashMap::new(),
        }
    }

    async fn document(&self) -> Result<Value> {
        let unavailable = || {
            error(
                "CONFIG_UNAVAILABLE",
                "アカウント設定を読み込めません。設定ファイルを確認してください。",
            )
        };
        // 読込み前にもサイズを確認し、異常な設定で巨大な領域を確保しない。
        let metadata = tokio::fs::metadata(&self.source)
            .await
            .map_err(|_| unavailable())?;
        if metadata.len() > MAX_BYTES as u64 {
            return Err(unavailable());
        }
        let bytes = tokio::fs::read(&self.source)
            .await
            .map_err(|_| unavailable())?;
        if bytes.len() > MAX_BYTES {
            return Err(unavailable());
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| unavailable())?;
        if !value.is_object() || !value["twitterapi_io"].is_object() {
            return Err(unavailable());
        }
        #[cfg(feature = "test-fixture")]
        crate::test_support::config_read(&self.source, &value);
        Ok(value)
    }

    fn profiles(secrets: &Value) -> &Map<String, Value> {
        secrets["twitterapi_io"].as_object().unwrap()
    }

    fn is_profile(name: &str, value: &Value) -> bool {
        name != "default_account" && value.is_object()
    }

    fn selected_name(
        &self,
        secrets: &Value,
        configs: &Map<String, Value>,
        account: Option<&str>,
    ) -> Result<String> {
        let selected = account
            .map(str::to_owned)
            .or_else(|| std::env::var("PLUMIVOX_DEFAULT_ACCOUNT").ok())
            .or_else(|| self.profile.clone());
        Self::resolve_name(secrets, configs, selected)
    }

    fn resolve_name(
        secrets: &Value,
        configs: &Map<String, Value>,
        mut selected: Option<String>,
    ) -> Result<String> {
        if selected.is_none() && secrets["twitterapi_io"].get("default_account").is_some() {
            let default = &secrets["twitterapi_io"]["default_account"];
            if !nonempty(default)
                || !configs
                    .get(default.as_str().unwrap_or(""))
                    .is_some_and(|v| Self::is_profile(default.as_str().unwrap_or(""), v))
            {
                return Err(error(
                    "ACCOUNT_DEFAULT_INVALID",
                    "既定アカウントの設定が不正です。default_accountに登録済みのアカウント設定名を指定してください。",
                ));
            }
            selected = default.as_str().map(str::to_owned);
        }
        if selected.is_none() {
            let mut profiles = configs
                .iter()
                .filter(|(name, value)| Self::is_profile(name, value));
            if let Some((name, _)) = profiles.next()
                && profiles.next().is_none()
            {
                selected = Some(name.clone());
            }
        }
        let selected = selected.filter(|s| !s.is_empty()).ok_or_else(|| {
            error(
                "ACCOUNT_REQUIRED",
                "アカウントを指定するか、セットアップで既定アカウントを設定してください。",
            )
        })?;
        if !configs
            .get(&selected)
            .is_some_and(|v| Self::is_profile(&selected, v))
        {
            return Err(error(
                "ACCOUNT_UNKNOWN",
                "指定されたアカウント設定がありません。",
            ));
        }
        Ok(selected)
    }

    async fn select(&self, name: Option<&str>) -> Result<(String, Value, Value)> {
        let secrets = self.document().await?;
        let profiles = Self::profiles(&secrets);
        let name = self.selected_name(&secrets, profiles, name)?;
        Ok((name.clone(), profiles[&name].clone(), secrets))
    }

    async fn inspect(&self, name: &str, config: &Value) -> Value {
        let mut result = normalized(name, config);
        let ready = read_ready(&result);
        let own = if ready {
            self.sessions.read(&result).await
        } else {
            json!({"status":"missing"})
        };
        let mut source = Value::Null;
        let mut acquired_at = Value::Null;
        if own["status"] == "valid" {
            result["loginCookies"] = own["cookie"].clone();
            source = json!(if cfg!(target_os = "macos") {
                "plumivox_keychain"
            } else {
                "plumivox_dpapi"
            });
            acquired_at = own["acquiredAt"].clone();
        } else if own["status"] == "missing" && ready {
            if nonempty(&config["login_cookies"]) {
                result["loginCookies"] = config["login_cookies"].clone();
                source = json!("configuration");
            } else if nonempty(&result["proxy"]) {
                let folder = match name {
                    "release_posting" => Some("TwitterApiRelease"),
                    "blog_posting" => Some("TwitterApiBlog"),
                    _ => None,
                };
                let directory = self.session_paths.get(name).cloned().or_else(|| {
                    folder.and_then(|f| {
                        std::env::var_os("LOCALAPPDATA")
                            .filter(|s| !s.is_empty())
                            .map(|p| PathBuf::from(p).join("Kagayoi").join(f))
                    })
                });
                if let Some(directory) = directory {
                    let user = result["userName"].as_str().unwrap();
                    let file = directory
                        .join(hash(&user.to_ascii_lowercase()))
                        .join("session.dpapi");
                    let binding = hash(&format!(
                        "{}\n{}",
                        result["apiKey"].as_str().unwrap(),
                        result["proxy"].as_str().unwrap()
                    ));
                    if let Some(cookie) = read_legacy_protected_cookie(&file, &binding, user)
                        .await
                        .filter(|c| !c.is_empty())
                    {
                        result["loginCookies"] = json!(cookie);
                        source = json!("windows_dpapi");
                    }
                }
            }
        }
        result["readConfigured"] = json!(ready);
        result["loginConfigured"] = json!(login_ready(config, &result));
        result["postConfigured"] =
            json!(ready && nonempty(&result["proxy"]) && nonempty(&result["loginCookies"]));
        result["sessionSource"] = source;
        result["sessionAcquiredAt"] = acquired_at;
        result
    }

    async fn metadata(&self, name: &str, config: &Value, default: Option<&str>) -> Value {
        let inspected = self.inspect(name, config).await;
        let mut result = json!({"account":name,"isDefault":Some(name)==default});
        for key in [
            "userName",
            "readConfigured",
            "postConfigured",
            "loginConfigured",
            "sessionSource",
            "sessionAcquiredAt",
        ] {
            result[key] = inspected[key].clone();
        }
        result["proxyMode"] = match config
            .get("proxy_mode")
            .filter(|v| !v.is_null())
            .and_then(Value::as_str)
        {
            None if config["proxy_mode"].is_null() => json!("external"),
            Some("external") => json!("external"),
            Some("managed") => json!("managed"),
            _ => json!("invalid"),
        };
        result
    }

    pub async fn list_accounts(&self) -> Result<Value> {
        let secrets = self.document().await?;
        let profiles = Self::profiles(&secrets);
        let default = self.selected_name(&secrets, profiles, None).ok();
        let mut accounts = Vec::new();
        for (name, config) in profiles
            .iter()
            .filter(|(name, value)| Self::is_profile(name, value))
        {
            accounts.push(self.metadata(name, config, default.as_deref()).await);
        }
        Ok(Value::Array(accounts))
    }

    pub async fn get_account_metadata(&self, name: Option<&str>) -> Result<Value> {
        let (name, config, secrets) = self.select(name).await?;
        let default = self
            .selected_name(&secrets, Self::profiles(&secrets), None)
            .ok();
        Ok(self.metadata(&name, &config, default.as_deref()).await)
    }

    /// 公開読取りはセッションの検査・読込み・復号を行わない。
    pub async fn get_read_account(&self, name: Option<&str>) -> Result<Value> {
        let (name, config, _) = self.select(name).await?;
        let mut result = normalized(&name, &config);
        if !read_ready(&result) {
            return Err(invalid_account());
        }
        result.as_object_mut().unwrap().remove("proxy");
        Ok(result)
    }

    pub async fn get_account(&self, name: Option<&str>, require_posting: bool) -> Result<Value> {
        let (name, config, _) = self.select(name).await?;
        let mut result = self.inspect(&name, &config).await;
        if result["readConfigured"] != true {
            return Err(invalid_account());
        }
        if require_posting && result["postConfigured"] != true {
            return Err(error(
                "POSTING_UNAVAILABLE",
                "投稿には有効なプロキシと一致するv2 Cookieが必要です。",
            ));
        }
        result.as_object_mut().unwrap().retain(|key, _| {
            ["account", "userName", "apiKey", "proxy", "loginCookies"].contains(&key.as_str())
        });
        Ok(result)
    }

    pub async fn get_login_credentials(&self, name: Option<&str>) -> Result<Value> {
        let (name, config, _) = self.select(name).await?;
        let mut result = normalized(&name, &config);
        if !valid_totp(&config["totp_secret"]) {
            return Err(error(
                "LOGIN_CREDENTIALS_INVALID",
                "totp_secretには認証コードではなくBase32の2FA秘密鍵を設定してください。",
            ));
        }
        if !login_ready(&config, &result) {
            return Err(error(
                "LOGIN_UNAVAILABLE",
                "ログインにはAPIキー、ユーザー名、プロキシ、メールアドレス、パスワードを設定してください。",
            ));
        }
        result["email"] = config["email"].clone();
        result["password"] = config["password"].clone();
        if nonempty(&config["totp_secret"]) {
            result["totpSecret"] = config["totp_secret"].clone();
        }
        Ok(result)
    }

    pub async fn get_proxy_configuration(
        &self,
        name: Option<&str>,
        expected_proxy: Option<&str>,
    ) -> Result<Value> {
        let (_, config, secrets) = self.select(name).await?;
        let mode = config
            .get("proxy_mode")
            .filter(|v| !v.is_null())
            .cloned()
            .unwrap_or(json!("external"));
        if mode != "external" && mode != "managed" {
            return Err(error(
                "PROXY_CONFIG",
                "proxy_modeはexternalまたはmanagedを指定してください。",
            ));
        }
        if expected_proxy.is_some_and(|expected| config["proxy"].as_str() != Some(expected)) {
            return Err(error(
                "AUTH_CONFIG_CHANGED",
                "通信の準備中にプロキシ設定が変更されました。再実行してください。",
            ));
        }
        if mode == "external" {
            return Ok(Value::Null);
        }
        let relay = &secrets["plumivox"]["managed_proxy"];
        let port = |key: &str, fallback: u64, minimum: u64| -> Option<u64> {
            if relay.get(key).is_none() {
                Some(fallback)
            } else {
                relay[key].as_u64().filter(|p| *p >= minimum && *p <= 65535)
            }
        };
        let ssh_port = if relay["ssh_port"].is_null() {
            Some(22)
        } else {
            port("ssh_port", 22, 1)
        };
        let local_port = port("local_port", 43128, 1024);
        let remote_port = port("remote_port", 43129, 1024);
        if !relay.is_object()
            || !valid_proxy(&config["proxy"])
            || !matches(r"\A[A-Za-z0-9._:\[\]-]{1,253}\z", &relay["ssh_host"])
            || !matches(r"\A[A-Za-z0-9._-]{1,64}\z", &relay["ssh_user"])
            || !nonempty(&relay["private_key_file"])
            || !relay["private_key_file"].as_str().is_some_and(|p| {
                PathBuf::from(p).is_absolute() && !p.chars().any(|c| c <= '\u{1f}')
            })
            || !matches(r"\ASHA256:[A-Za-z0-9+/]{43}\z", &relay["host_key_sha256"])
            || ssh_port.is_none()
            || local_port.is_none()
            || remote_port.is_none()
        {
            return Err(error(
                "PROXY_CONFIG",
                "Plumivoxの中継設定に接続先、SSH利用者、秘密鍵ファイル、ホスト鍵指紋、有効なポートを設定してください。",
            ));
        }
        let proxy = url::Url::parse(config["proxy"].as_str().unwrap()).unwrap();
        if proxy.host_str() != relay["ssh_host"].as_str()
            || proxy.port().is_none()
            || proxy.path() != "/"
            || proxy.query().is_some()
            || proxy.username().is_empty()
            || proxy.password().is_none_or(str::is_empty)
        {
            return Err(error(
                "PROXY_CONFIG",
                "managedモードのproxyにはSSH接続先と同じホストの認証付き公開プロキシを設定してください。",
            ));
        }
        crate::proxy::normalize_proxy_config(
            &json!({"sshHost":relay["ssh_host"],"sshPort":ssh_port.unwrap(),
            "sshUser":relay["ssh_user"],"privateKeyFile":relay["private_key_file"],"hostKeySha256":relay["host_key_sha256"],
            "localPort":local_port.unwrap(),"remotePort":remote_port.unwrap()}),
        )
    }

    pub async fn lock_session(&self, name: Option<&str>) -> Result<(Value, SessionLease)> {
        let (name, config, _) = self.select(name).await?;
        let account = normalized(&name, &config);
        if !read_ready(&account) {
            return Err(invalid_account());
        }
        let account = json!({"account": name,"userName":account["userName"]});
        let lease = self.sessions.lock(&account).await?;
        Ok((account, lease))
    }

    pub async fn prepare_session_storage(&self, name: Option<&str>) -> Result<()> {
        let credentials = self.get_login_credentials(name).await?;
        self.sessions.prepare(&credentials).await
    }

    pub async fn save_session(
        &self,
        name: Option<&str>,
        cookie: &str,
        expected_account: &Value,
        lease: &SessionLease,
        cancel: CancellationToken,
    ) -> Result<Value> {
        let credentials = self
            .get_login_credentials(name)
            .await
            .map_err(|_| changed())?;
        if ["account", "userName", "apiKey", "proxy"]
            .iter()
            .any(|key| credentials[*key] != expected_account[*key])
        {
            return Err(changed());
        }
        self.sessions
            .save(&credentials, cookie, lease, cancel)
            .await
    }
}
