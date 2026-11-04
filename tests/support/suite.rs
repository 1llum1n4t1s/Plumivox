use super::Result;
use anyhow::{Context, bail, ensure};
use serde_json::{Value, json};
use std::{
    fs,
    future::Future,
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

pub fn redact(text: &str, secrets: &[String]) -> String {
    // 包含関係のある秘密も、長い値から伏せて一部を残さない。
    let mut ordered: Vec<_> = secrets.iter().filter(|secret| !secret.is_empty()).collect();
    ordered.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
    ordered.into_iter().fold(text.to_owned(), |text, secret| {
        text.replace(secret, "[redacted]")
    })
}

fn no_links(path: &Path) -> Result<()> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                ensure!(
                    !metadata.file_type().is_symlink(),
                    "Evidence path crosses a symbolic link"
                );
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    ensure!(
                        metadata.file_attributes() & 0x400 == 0,
                        "Evidence path crosses a reparse point"
                    );
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("Cannot inspect evidence path"),
        }
    }
    Ok(())
}

fn contains(child: &Path, parent: &Path) -> bool {
    #[cfg(windows)]
    {
        let child = child.to_string_lossy().replace('/', "\\").to_lowercase();
        let parent = parent.to_string_lossy().replace('/', "\\").to_lowercase();
        child == parent || child.starts_with(&format!("{}\\", parent.trim_end_matches('\\')))
    }
    #[cfg(not(windows))]
    {
        child.starts_with(parent)
    }
}

fn validate_private_directory(path: &Path) -> Result<()> {
    no_links(path)?;
    let metadata = fs::symlink_metadata(path).context("Cannot inspect evidence directory")?;
    ensure!(
        metadata.is_dir() && !metadata.permissions().readonly(),
        "Evidence directory is not writable"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            metadata.uid() == unsafe { libc::geteuid() },
            "Evidence directory must be owned by the current user"
        );
        ensure!(
            metadata.mode() & 0o077 == 0,
            "Existing evidence directory must not grant group or other access"
        );
    }
    #[cfg(windows)]
    windows_owner::validate(path)?;
    Ok(())
}

#[cfg(windows)]
mod windows_owner {
    use super::*;
    use std::{os::windows::ffi::OsStrExt, ptr::null_mut};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, LocalFree},
        Security::{Authorization::*, *},
        Storage::FileSystem::CreateDirectoryW,
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    struct Token(HANDLE);
    impl Drop for Token {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    struct Descriptor(*mut core::ffi::c_void);
    impl Drop for Descriptor {
        fn drop(&mut self) {
            unsafe {
                LocalFree(self.0);
            }
        }
    }

    fn current_user() -> Result<Vec<usize>> {
        unsafe {
            let mut token = null_mut();
            ensure!(
                OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) != 0,
                "Cannot inspect current Windows user"
            );
            let token = Token(token);
            let mut size = 0;
            GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut size);
            ensure!(
                size != 0 && size <= 65536,
                "Cannot inspect current Windows user SID"
            );
            let mut buffer = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
            ensure!(
                GetTokenInformation(
                    token.0,
                    TokenUser,
                    buffer.as_mut_ptr().cast(),
                    size,
                    &mut size
                ) != 0,
                "Cannot read current Windows user SID"
            );
            Ok(buffer)
        }
    }

    pub(super) fn validate(path: &Path) -> Result<()> {
        check(path, true)
    }

    fn check(path: &Path, check_acl: bool) -> Result<()> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let user = current_user()?;
        unsafe {
            let sid = (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid;
            let mut owner = null_mut();
            let mut dacl = null_mut();
            let mut descriptor = null_mut();
            ensure!(
                GetNamedSecurityInfoW(
                    wide.as_ptr(),
                    SE_FILE_OBJECT,
                    OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                    &mut owner,
                    null_mut(),
                    &mut dacl,
                    null_mut(),
                    &mut descriptor
                ) == 0,
                "Cannot inspect Windows evidence directory owner"
            );
            let _descriptor = Descriptor(descriptor);
            ensure!(
                !owner.is_null() && EqualSid(owner, sid) != 0,
                "Evidence directory must be owned by the current Windows user"
            );
            if check_acl {
                ensure!(
                    !dacl.is_null(),
                    "Evidence directory must not have an unrestricted Windows DACL"
                );
                for index in 0..u32::from((*dacl).AceCount) {
                    let mut ace = null_mut();
                    ensure!(
                        GetAce(dacl, index, &mut ace) != 0,
                        "Cannot inspect evidence directory Windows DACL"
                    );
                    let header = &*ace.cast::<ACE_HEADER>();
                    if header.AceType == 1 {
                        continue;
                    } // 拒否ACEはアクセスを広げない。
                    ensure!(
                        header.AceType == 0,
                        "Evidence directory has an unsupported Windows DACL entry"
                    );
                    let allowed = &*ace.cast::<ACCESS_ALLOWED_ACE>();
                    let principal = (&allowed.SidStart as *const u32).cast_mut().cast();
                    ensure!(
                        allowed.Mask == 0
                            || EqualSid(principal, sid) != 0
                            || IsWellKnownSid(principal, WinLocalSystemSid) != 0
                            || IsWellKnownSid(principal, WinBuiltinAdministratorsSid) != 0,
                        "Evidence directory Windows DACL grants shared access"
                    );
                }
            }
        }
        Ok(())
    }

    pub(super) fn create_new(path: &Path) -> Result<bool> {
        // 作成時から本人限定にし、親の共有ACLを一瞬でも継承しない。
        no_links(path)?;
        let user = current_user()?;
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            let sid = (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid;
            let mut sid_text = null_mut();
            ensure!(
                ConvertSidToStringSidW(sid, &mut sid_text) != 0,
                "Cannot encode evidence owner SID"
            );
            let _sid_text = Descriptor(sid_text.cast());
            let mut length = 0;
            while *sid_text.add(length) != 0 {
                length += 1;
            }
            let sid_text = String::from_utf16(std::slice::from_raw_parts(sid_text, length))
                .context("Cannot encode evidence owner SID")?;
            let sddl: Vec<u16> = format!("O:{sid_text}D:P(A;OICI;FA;;;{sid_text})")
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let mut descriptor = null_mut();
            ensure!(
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    1,
                    &mut descriptor,
                    null_mut()
                ) != 0,
                "Cannot create private evidence Windows DACL"
            );
            let _descriptor = Descriptor(descriptor);
            let mut present = 0;
            let mut defaulted = 0;
            let mut dacl = null_mut();
            ensure!(
                GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted) != 0
                    && present != 0
                    && !dacl.is_null(),
                "Cannot inspect private evidence Windows DACL"
            );
            let attributes = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor,
                bInheritHandle: 0,
            };
            if CreateDirectoryW(wide.as_ptr(), &attributes) == 0 {
                let error = GetLastError();
                // 競合先は既存の扱いへ戻し、所有者やACLを一切変更しない。
                if error == ERROR_ALREADY_EXISTS {
                    return Ok(false);
                }
                bail!("Cannot atomically create private evidence Windows directory");
            }
            let mut actual = null_mut();
            ensure!(
                GetNamedSecurityInfoW(
                    wide.as_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    &mut actual
                ) == 0,
                "Cannot verify new evidence Windows DACL"
            );
            let _actual = Descriptor(actual);
            let mut control = 0;
            let mut revision = 0;
            ensure!(
                GetSecurityDescriptorControl(actual, &mut control, &mut revision) != 0
                    && control & SE_DACL_PROTECTED != 0,
                "New evidence Windows DACL inheritance is not protected"
            );
        }
        validate(path)?;
        Ok(true)
    }
}

pub struct Suite {
    output: PathBuf,
    label: String,
    secrets: Vec<String>,
    results: Vec<Value>,
    #[cfg(windows)]
    _directory_lease: fs::File,
}

impl Suite {
    pub fn new(envkey: &str, label: &str) -> Result<Self> {
        let output = if let Some(path) = std::env::var_os(envkey).filter(|value| !value.is_empty())
        {
            PathBuf::from(path)
        } else {
            #[cfg(windows)]
            let root = {
                // 元のパス表記を保ち、正規化した実体との照合はnew_at内で行う。
                let root = std::env::temp_dir();
                ensure!(root.is_absolute(), "Test temp directory must be absolute");
                root
            };
            #[cfg(not(windows))]
            let root = fs::canonicalize(std::env::temp_dir())
                .context("Cannot resolve test temp directory")?;
            let mut bytes = rand::random::<[u8; 16]>();
            bytes[6] = (bytes[6] & 0x0f) | 0x40;
            bytes[8] = (bytes[8] & 0x3f) | 0x80;
            let id = hex::encode(bytes);
            root.join("Codex").join("Plumivox").join(format!(
                "{}-{}-{}-{}-{}",
                &id[..8],
                &id[8..12],
                &id[12..16],
                &id[16..20],
                &id[20..]
            ))
        };
        Self::new_at(output, label)
    }

    pub fn new_at(output: PathBuf, label: &str) -> Result<Self> {
        ensure!(
            output.is_absolute()
                && !output
                    .components()
                    .any(|part| matches!(part, Component::ParentDir)),
            "Evidence output must be an absolute path without parent traversal"
        );
        ensure!(
            output.parent().is_some()
                && output
                    .components()
                    .any(|part| matches!(part, Component::Normal(_))),
            "Evidence output must not be a filesystem root"
        );
        no_links(&output)?;
        let mut ancestor = output.as_path();
        while !ancestor.exists() {
            ancestor = ancestor.parent().context("Missing evidence parent")?;
        }
        let resolved = fs::canonicalize(ancestor)?.join(output.strip_prefix(ancestor)?);
        ensure!(
            resolved.parent().is_some()
                && resolved
                    .components()
                    .any(|part| matches!(part, Component::Normal(_))),
            "Resolved evidence output must not be a filesystem root"
        );
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut blocked = vec![repo.clone(), fs::canonicalize(repo)?];
        if let Some(home) = dirs::home_dir() {
            blocked.push(home.join(".codex"));
        }
        if let Some(home) = std::env::var_os("CODEX_HOME").filter(|s| !s.is_empty()) {
            blocked.push(home.into());
        }
        for root in blocked {
            ensure!(
                !contains(&output, &root) && !contains(&resolved, &root),
                "Evidence output must remain outside repository and Codex user directories"
            );
            if let Ok(canonical) = root.canonicalize() {
                ensure!(
                    !contains(&resolved, &canonical),
                    "Evidence output resolves into a protected directory"
                );
            }
        }
        if output.exists() {
            // 所有者・公開権限を確認する前に既存ディレクトリへ書き込まない。
            // 権限変更や所有者変更による救済も行わない。
            validate_private_directory(&output)?;
            ensure!(
                fs::read_dir(&output)?.next().is_none(),
                "Evidence output must be empty to avoid overwriting prior data"
            );
        } else {
            let parent = output.parent().context("Missing evidence output parent")?;
            let mut parents = fs::DirBuilder::new();
            parents.recursive(true);
            #[cfg(unix)]
            let mut builder = fs::DirBuilder::new();
            #[cfg(not(any(unix, windows)))]
            let builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                parents.mode(0o700);
                builder.mode(0o700);
            }
            parents
                .create(parent)
                .context("Cannot create evidence parent directory")?;
            #[cfg(windows)]
            {
                windows_owner::create_new(&output)?;
            }
            #[cfg(not(windows))]
            match builder.create(&output) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    // 作成競合した既存ディレクトリのACLは変更しない。
                    validate_private_directory(&output)?;
                    ensure!(
                        fs::read_dir(&output)?.next().is_none(),
                        "Evidence output must be empty to avoid overwriting prior data"
                    );
                }
                Err(error) => return Err(error).context("Cannot create test evidence directory"),
            }
        }
        #[cfg(windows)]
        let directory_lease = {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::{
                FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
                FILE_SHARE_WRITE,
            };
            // GENERIC_READを伴うdirectory handleを保持し、DELETE共有は許さない。
            // 親のFILE_DELETE_CHILDで出力先・祖先を置換する経路もSuiteの生存中は塞ぐ。
            fs::OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(&output)
                .context("Cannot hold evidence directory against replacement")?
        };
        // leaseを取得した後に全祖先・owner/DACL・非空を再確認する。
        validate_private_directory(&output)?;
        ensure!(
            fs::read_dir(&output)?.next().is_none(),
            "Evidence output must be empty to avoid overwriting prior data"
        );
        Ok(Self {
            output,
            label: label.into(),
            secrets: Vec::new(),
            results: Vec::new(),
            #[cfg(windows)]
            _directory_lease: directory_lease,
        })
    }

    pub fn output(&self) -> &Path {
        &self.output
    }
    pub fn add_secret(&mut self, secret: String) {
        if !secret.is_empty() {
            self.secrets.push(secret);
        }
    }
    pub fn redact(&self, text: &str) -> String {
        redact(text, &self.secrets)
    }

    fn redact_value(&self, value: &Value) -> Value {
        match value {
            Value::String(text) => Value::String(self.redact(text)),
            Value::Array(items) => {
                Value::Array(items.iter().map(|item| self.redact_value(item)).collect())
            }
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(key, value)| (self.redact(key), self.redact_value(value)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    pub fn record(&mut self, name: impl AsRef<str>, result: Result<()>) -> bool {
        let name = self.redact(name.as_ref());
        let passed = result.is_ok();
        self.results.push(match result {
            Ok(()) => json!({"name":name,"ok":true}),
            Err(error) => {
                json!({"name":name,"ok":false,"error":self.redact(&format!("{error:#}"))})
            }
        });
        passed
    }

    pub fn write_json(&self, name: &str, value: &Value) -> Result<()> {
        validate_private_directory(&self.output)?;
        let relative = Path::new(name);
        ensure!(
            !relative.is_absolute()
                && relative
                    .components()
                    .all(|part| matches!(part, Component::Normal(_))),
            "Evidence filename must remain inside suite output"
        );
        let destination = self.output.join(relative);
        no_links(&destination)?;
        let bytes = serde_json::to_string_pretty(&self.redact_value(value))?;
        use std::io::Write;
        let mut options = fs::OpenOptions::new();
        options.create(true).truncate(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        options.open(destination)?.write_all(bytes.as_bytes())?;
        Ok(())
    }

    pub fn finish(&self) -> Result<()> {
        let failed = self
            .results
            .iter()
            .filter(|result| result["ok"] == false)
            .count();
        let summary = json!({"label":self.label,"output":self.output,"total":self.results.len(),
            "passed":self.results.len()-failed,"failed":failed,"results":self.results});
        self.write_json("summary.json", &summary)?;
        println!("{}", self.redact_value(&summary));
        ensure!(
            failed == 0,
            "{} E2E cases failed; inspect suite summary",
            failed
        );
        Ok(())
    }
}

pub fn free_port() -> Result<u16> {
    Ok(
        std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?
            .local_addr()?
            .port(),
    )
}

pub async fn wait_until<F, Fut>(timeout: Duration, mut condition: F) -> Result<()>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<bool>>,
{
    let deadline = Instant::now() + timeout;
    loop {
        if condition().await? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!("Timed out waiting for E2E condition");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
