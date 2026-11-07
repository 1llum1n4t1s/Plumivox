use crate::{Result, root};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::{Read, Seek, Write},
    path::Path,
    process::{Command, Stdio},
};

const MAX_FILE: usize = 256 * 1024 * 1024;
const MAX_MANIFEST: usize = 16 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Identity {
    commit: String,
    run: String,
    attempt: String,
    target: String,
}
impl Identity {
    fn validate(&self) -> Result {
        if self.commit.len() != 40
            || !self
                .commit
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err("commitは小文字40桁のGit SHAを指定してください".into());
        }
        for value in [&self.run, &self.attempt] {
            if value.is_empty()
                || value.len() > 20
                || value.starts_with('0')
                || !value.bytes().all(|b| b.is_ascii_digit())
            {
                return Err("run/attemptは正整数を指定してください".into());
            }
        }
        self.names()?;
        Ok(())
    }
    fn names(&self) -> Result<[&'static str; 2]> {
        Ok([
            match self.target.as_str() {
                "windows-x64" => "plumivox.exe",
                "macos-arm64" | "macos-universal" => "plumivox",
                "vps-linux-x86_64" => "plumivox-vps-setup-x86_64",
                "vps-linux-aarch64" => "plumivox-vps-setup-aarch64",
                _ => return Err("targetが許可された配布対象ではありません".into()),
            },
            "THIRD-PARTY-LICENSES.txt",
        ])
    }
    fn prefix(&self) -> String {
        format!(
            "plumivox/ci/{}/{}/{}/{}/",
            self.commit, self.run, self.attempt, self.target
        )
    }

    fn source(&self, repository: &Path, name: &str) -> std::path::PathBuf {
        if self.target.starts_with("vps-linux-") && name != "THIRD-PARTY-LICENSES.txt" {
            repository.join("setup/vps").join(name)
        } else {
            repository.join("dist").join(name)
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    name: String,
    size: u64,
    sha256: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,
    identity: Identity,
    files: Vec<Entry>,
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn verify(entry: &Entry, bytes: &[u8]) -> Result {
    if bytes.len() as u64 != entry.size || digest(bytes) != entry.sha256 {
        return Err("成果物のサイズまたはSHA256が一致しません".into());
    }
    Ok(())
}
fn manifest(bytes: &[u8], identity: &Identity) -> Result<Manifest> {
    if bytes.len() > MAX_MANIFEST {
        return Err("manifestが上限を超えています".into());
    }
    let m: Manifest = serde_json::from_slice(bytes).map_err(|_| "manifestの形式が不正です")?;
    if m.schema != 1 || m.identity != *identity || m.files.len() != 2 {
        return Err("manifestのidentityまたはschemaが一致しません".into());
    }
    for (entry, name) in m.files.iter().zip(identity.names()?) {
        if entry.name != name
            || entry.size == 0
            || entry.size > MAX_FILE as u64
            || entry.sha256.len() != 64
            || !entry
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("manifestに未知または不正なファイルがあります".into());
        }
    }
    Ok(m)
}

// curl設定の文字列としてのみ使用し、資格情報をargvやファイルへ出さない。
fn quote(value: &str) -> Result<String> {
    if value.chars().any(|c| c.is_control()) {
        return Err("R2設定に制御文字は使用できません".into());
    }
    Ok(format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}
struct Store {
    endpoint: String,
    user: String,
}
impl Store {
    fn from_env() -> Result<Self> {
        let get = |name: &str| {
            env::var(format!("PLUMIVOX_R2_{name}"))
                .map_err(|_| format!("PLUMIVOX_R2_{name}が必要です"))
        };
        let account = get("ACCOUNT_ID")?;
        let bucket = get("BUCKET")?;
        let access = get("ACCESS_KEY_ID")?;
        let secret = get("SECRET_ACCESS_KEY")?;
        if account.len() != 32
            || !account.bytes().all(|b| b.is_ascii_hexdigit())
            || bucket.len() < 3
            || bucket.len() > 63
            || !bucket
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            || bucket.starts_with('-')
            || bucket.ends_with('-')
        {
            return Err("R2 accountまたはbucketが不正です".into());
        }
        if access.is_empty() || secret.is_empty() || access.contains(':') {
            return Err("R2資格情報が不正です".into());
        }
        let user = format!("{access}:{secret}");
        quote(&user)?;
        Ok(Self {
            endpoint: format!("https://{account}.r2.cloudflarestorage.com/{bucket}/"),
            user,
        })
    }
    fn request(
        &self,
        key: &str,
        upload: Option<&Path>,
        body: Option<&[u8]>,
        limit: usize,
    ) -> Result<Vec<u8>> {
        let mut config = format!(
            "url = {}\nuser = {}\n",
            quote(&format!("{}{key}", self.endpoint))?,
            quote(&self.user)?
        );
        if let Some(path) = upload {
            config.push_str(&format!(
                "upload-file = {}\n",
                quote(path.to_str().ok_or("成果物パスがUTF-8ではありません")?)?
            ));
        }
        if let Some(body) = body {
            config.push_str(&format!(
                "request = \"PUT\"\ndata-binary = {}\n",
                quote(std::str::from_utf8(body)?)?
            ));
        }
        if upload.is_some() || body.is_some() {
            config.push_str("header = \"If-None-Match: *\"\n");
        }
        let protocol = if cfg!(test) && self.endpoint.starts_with("http://127.0.0.1:") {
            "=http"
        } else {
            "=https"
        };
        let mut child = Command::new("curl")
            .args([
                "-q",
                "--config",
                "-",
                "--silent",
                "--fail",
                "--proto",
                protocol,
                "--noproxy",
                "*",
                "--aws-sigv4",
                "aws:amz:auto:s3",
                "--connect-timeout",
                "20",
                "--max-time",
                "300",
                "--write-out",
                "\n%{http_code}",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "curlを起動できません（AWS SigV4対応curlが必要です）")?;
        let write_result = child.stdin.take().unwrap().write_all(config.as_bytes());
        if write_result.is_err() {
            let _ = child.kill();
            child.wait().map_err(|_| "R2転送プロセスを回収できません")?;
            return Err("R2転送設定を渡せません".into());
        }
        // 本文上限と末尾のHTTP status 4 byteに、超過検知用の1 byteだけを加える。
        // curlの版やContent-Lengthの有無によらず、Rust側でメモリ使用を制限する。
        let mut bytes = Vec::new();
        let read_result = child
            .stdout
            .take()
            .unwrap()
            .take(limit as u64 + 5)
            .read_to_end(&mut bytes);
        if read_result.is_err() || bytes.len() > limit + 4 {
            let _ = child.kill();
            child.wait().map_err(|_| "R2転送プロセスを回収できません")?;
            return Err(if read_result.is_err() {
                "R2応答を読み取れません"
            } else {
                "R2応答がサイズ上限を超えています"
            }
            .into());
        }
        let status = child
            .wait()
            .map_err(|_| "R2転送プロセスの完了を確認できません")?;
        let success =
            bytes.len() >= 4 && matches!(&bytes[bytes.len() - 4..], b"\n200" | b"\n201" | b"\n204");
        if !status.success() || !success {
            return Err("R2転送失敗（認証・権限・接続・objectの存在を確認してください）".into());
        }
        let bytes = bytes[..bytes.len() - 4].to_vec();
        if bytes.len() > limit {
            return Err("R2応答がサイズ上限を超えています".into());
        }
        Ok(bytes)
    }
}

pub fn execute(args: &[String]) -> Result {
    let download = args[0] == "artifact-download";
    let mut options = std::collections::BTreeMap::new();
    for pair in args[1..].chunks(2) {
        if pair.len() != 2
            || !matches!(
                pair[0].as_str(),
                "--commit" | "--run" | "--attempt" | "--target" | "--output"
            )
            || options.insert(pair[0].as_str(), pair[1].as_str()).is_some()
        {
            return Err("成果物コマンドの引数が不正です".into());
        }
    }
    let get = |key| {
        options
            .get(key)
            .map(|s| s.to_string())
            .ok_or_else(|| format!("{key}が必要です"))
    };
    let identity = Identity {
        commit: get("--commit")?,
        run: get("--run")?,
        attempt: get("--attempt")?,
        target: get("--target")?,
    };
    identity.validate()?;
    if download != options.contains_key("--output") {
        return Err("downloadだけに--output（新規フォルダー）を指定してください".into());
    }
    let store = Store::from_env()?;
    transfer(
        &identity,
        &store,
        download
            .then(|| get("--output"))
            .transpose()?
            .as_deref()
            .map(Path::new),
    )
}

fn transfer(identity: &Identity, store: &Store, output: Option<&Path>) -> Result {
    transfer_from(identity, store, output, &root())
}

fn create_output(output: &Path) -> Result<(std::path::PathBuf, Vec<fs::File>)> {
    let absolute = if output.is_absolute() {
        output.to_path_buf()
    } else {
        env::current_dir()?.join(output)
    };
    if absolute
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err("出力先に親参照は使用できません".into());
    }
    let mut leases = Vec::new();
    #[cfg(unix)]
    let uid: u32 = {
        let output = Command::new("id")
            .arg("-u")
            .output()
            .map_err(|_| "出力先所有者の確認にidコマンドが必要です")?;
        if !output.status.success() {
            return Err("本人のuidを確認できません".into());
        }
        std::str::from_utf8(&output.stdout)?.trim().parse()?
    };
    // 本人が管理する親階層を前提とする。同じuidで動く別プロセスによる変更までは防がない。
    for ancestor in absolute
        .ancestors()
        .skip(1)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        let metadata = fs::symlink_metadata(ancestor)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("出力先の親はリンクでない既存ディレクトリが必要です".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let mode = metadata.mode();
            if (metadata.uid() != uid && metadata.uid() != 0)
                || (mode & 0o022 != 0 && mode & 0o1000 == 0)
            {
                return Err(
                    "出力先の親は本人またはroot所有で、他者が置換できない必要があります".into(),
                );
            }
        }
        #[cfg(windows)]
        leases.push(lease_directory(ancestor)?);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        builder
            .create(&absolute)
            .map_err(|_| "出力先は未存在で、親フォルダーが存在する必要があります")?;
    }
    #[cfg(windows)]
    {
        windows_private::create(&absolute)?;
        let lease = lease_directory(&absolute)?;
        windows_private::validate(&lease, true)?;
        if fs::read_dir(&absolute)?.next().is_some() {
            return Err("新規出力先が空ではありません".into());
        }
        leases.push(lease);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = fs::symlink_metadata(&absolute)?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || metadata.uid() != uid
            || metadata.mode() & 0o077 != 0
        {
            return Err("出力先の所有者または専用権限を確認できません".into());
        }
        leases.push(fs::File::open(&absolute)?);
    }
    Ok((absolute, leases))
}

#[cfg(windows)]
fn lease_directory(path: &Path) -> Result<fs::File> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    // READ/WRITE共有だけを許可し、親から順に保持してrenameとreparse差替を防ぐ。
    let file = fs::OpenOptions::new()
        .read(true)
        .share_mode(3)
        .custom_flags(0x02000000 | 0x00200000)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
        return Err("出力先のreparse pointは使用できません".into());
    }
    Ok(file)
}

fn write_artifact(directory: &Path, name: &str, data: &[u8]) -> Result {
    #[cfg(windows)]
    let mut file = windows_private::create_file(&directory.join(name))?;
    #[cfg(not(windows))]
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(directory.join(name))?;
    file.write_all(data)?;
    file.flush()?;
    #[cfg(windows)]
    windows_private::validate(&file, true)?;
    #[cfg(unix)]
    if name == "plumivox" || crate::VPS_BINARIES.iter().any(|entry| entry.1 == name) {
        set_executable(&file)?;
    }
    file.rewind()?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 8192];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    if file.metadata()?.len() != data.len() as u64
        || hash.finalize().as_slice() != Sha256::digest(data).as_slice()
    {
        return Err("保存後の成果物が一致しません".into());
    }
    Ok(())
}

#[cfg(windows)]
mod windows_private {
    use super::*;
    use std::{
        os::windows::{
            ffi::OsStrExt,
            io::{AsRawHandle, FromRawHandle},
        },
        ptr::null_mut,
    };
    use windows_sys::Win32::{
        Foundation::{
            CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, LocalFree,
        },
        Security::{Authorization::*, *},
        Storage::FileSystem::{
            CREATE_NEW, CreateDirectoryW, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ,
        },
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
    struct Allocation(*mut core::ffi::c_void);
    impl Drop for Allocation {
        fn drop(&mut self) {
            unsafe {
                LocalFree(self.0);
            }
        }
    }
    fn user() -> Result<Vec<usize>> {
        unsafe {
            let mut token = null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return Err("本人のWindows tokenを確認できません".into());
            }
            let token = Token(token);
            let mut size = 0;
            GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut size);
            if size == 0 || size > 65536 {
                return Err("本人のWindows SIDを確認できません".into());
            }
            let mut buffer = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
            if GetTokenInformation(
                token.0,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                size,
                &mut size,
            ) == 0
            {
                return Err("本人のWindows SIDを読み取れません".into());
            }
            Ok(buffer)
        }
    }
    pub(super) fn create(path: &Path) -> Result {
        create_with_acl(path, false)
    }
    fn create_with_acl(path: &Path, shared: bool) -> Result {
        let wide = path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let descriptor = descriptor(shared)?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: 0,
        };
        // 親ACLを継承しない新規作成だけを許可し、既存の所有者・ACLを修復しない。
        if unsafe { CreateDirectoryW(wide.as_ptr(), &attributes) } == 0 {
            return Err("本人限定の新規出力先を作成できません（既存先は使用できません）".into());
        }
        Ok(())
    }
    pub(super) fn create_file(path: &Path) -> Result<fs::File> {
        let wide = path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let descriptor = descriptor(false)?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: 0,
        };
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ,
                &attributes,
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(
                "本人限定の新規成果物を作成できません（既存ファイルは使用できません）".into(),
            );
        }
        // 成功したhandleの所有権をFileへ一度だけ移し、以降の失敗時もRAIIで回収する。
        Ok(unsafe { fs::File::from_raw_handle(handle) })
    }
    fn descriptor(shared: bool) -> Result<Allocation> {
        let user = user()?;
        unsafe {
            let sid = (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid;
            let mut text = null_mut();
            if ConvertSidToStringSidW(sid, &mut text) == 0 {
                return Err("本人SIDを変換できません".into());
            }
            let _text = Allocation(text.cast());
            let mut length = 0;
            while *text.add(length) != 0 {
                length += 1;
            }
            let text = String::from_utf16(std::slice::from_raw_parts(text, length))?;
            let extra = if shared { "(A;OICI;FA;;;WD)" } else { "" };
            let sddl = format!("O:{text}D:P(A;OICI;FA;;;{text}){extra}")
                .encode_utf16()
                .chain(Some(0))
                .collect::<Vec<_>>();
            let mut descriptor = null_mut();
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                null_mut(),
            ) == 0
            {
                return Err("本人限定DACLを構築できません".into());
            }
            Ok(Allocation(descriptor))
        }
    }
    pub(super) fn validate(file: &fs::File, protected: bool) -> Result {
        let user = user()?;
        unsafe {
            let sid = (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid;
            let mut owner = null_mut();
            let mut dacl = null_mut();
            let mut descriptor = null_mut();
            if GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                &mut dacl,
                null_mut(),
                &mut descriptor,
            ) != 0
            {
                return Err("出力先の所有者・DACLを確認できません".into());
            }
            let _descriptor = Allocation(descriptor);
            if owner.is_null()
                || EqualSid(owner, sid) == 0
                || dacl.is_null()
                || (*dacl).AceCount != 1
            {
                return Err("出力先は本人所有・本人限定DACLである必要があります".into());
            }
            let mut control = 0;
            let mut revision = 0;
            if GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) == 0
                || (protected && control & SE_DACL_PROTECTED == 0)
            {
                return Err("出力先DACLの継承保護を確認できません".into());
            }
            let mut ace = null_mut();
            if GetAce(dacl, 0, &mut ace) == 0 || (*ace.cast::<ACE_HEADER>()).AceType != 0 {
                return Err("出力先DACLの許可項目が不正です".into());
            }
            let allowed = &*ace.cast::<ACCESS_ALLOWED_ACE>();
            let principal = (&allowed.SidStart as *const u32).cast_mut().cast();
            if EqualSid(principal, sid) == 0 || allowed.Mask != 0x001f01ff {
                return Err("出力先DACLは本人だけのfull accessである必要があります".into());
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn shared_parent(path: &Path) -> Result {
        create_with_acl(path, true)
    }

    #[cfg(test)]
    pub(super) fn security_text(file: &fs::File) -> Result<String> {
        unsafe {
            let mut descriptor = null_mut();
            let flags = OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
            if GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                flags,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
                &mut descriptor,
            ) != 0
            {
                return Err("fixture DACL inspection failed".into());
            }
            let _descriptor = Allocation(descriptor);
            let mut text = null_mut();
            if ConvertSecurityDescriptorToStringSecurityDescriptorW(
                descriptor,
                1,
                flags,
                &mut text,
                null_mut(),
            ) == 0
            {
                return Err("fixture DACL conversion failed".into());
            }
            let _text = Allocation(text.cast());
            let mut length = 0;
            while *text.add(length) != 0 {
                length += 1;
            }
            Ok(String::from_utf16(std::slice::from_raw_parts(
                text, length,
            ))?)
        }
    }
}

#[cfg(unix)]
fn set_executable(file: &fs::File) -> Result {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    file.set_permissions(fs::Permissions::from_mode(0o755))?;
    if file.metadata()?.mode() & 0o777 != 0o755 {
        return Err("実行権限の適用を確認できません".into());
    }
    Ok(())
}

fn transfer_from(
    identity: &Identity,
    store: &Store,
    output: Option<&Path>,
    repository: &Path,
) -> Result {
    if output.is_none() && identity.target == "macos-universal" {
        return Err("旧macOS universal成果物は読取り専用です".into());
    }
    let prefix = identity.prefix();
    if let Some(output) = output {
        let bytes = store.request(&format!("{prefix}manifest.json"), None, None, MAX_MANIFEST)?;
        let m = manifest(&bytes, identity)?;
        // 全ファイルを検証してから、新規出力先へ保存する。
        let files = m
            .files
            .iter()
            .map(|entry| {
                let data = store.request(
                    &format!("{prefix}{}", entry.name),
                    None,
                    None,
                    entry.size as usize,
                )?;
                verify(entry, &data)?;
                Ok(data)
            })
            .collect::<Result<Vec<_>>>()?;
        let (absolute, _directory_leases) = create_output(output)?;
        for (entry, data) in m.files.iter().zip(files) {
            write_artifact(&absolute, &entry.name, &data)?;
        }
        write_artifact(&absolute, "manifest.json", &bytes)?;
    } else {
        let mut m = Manifest {
            schema: 1,
            identity: identity.clone(),
            files: Vec::new(),
        };
        for name in identity.names()? {
            let path = identity.source(repository, name);
            let meta = fs::symlink_metadata(&path)?;
            if !meta.file_type().is_file() || meta.len() == 0 || meta.len() > MAX_FILE as u64 {
                return Err("配布成果物が通常ファイルではないかサイズが不正です".into());
            }
            let data = fs::read(&path)?;
            m.files.push(Entry {
                name: name.into(),
                size: data.len() as u64,
                sha256: digest(&data),
            });
        }
        for entry in &m.files {
            let key = format!("{prefix}{}", entry.name);
            store.request(
                &key,
                Some(&identity.source(repository, &entry.name)),
                None,
                MAX_MANIFEST,
            )?;
            verify(
                entry,
                &store.request(&key, None, None, entry.size as usize)?,
            )?;
        }
        let bytes = serde_json::to_vec(&m)?;
        let key = format!("{prefix}manifest.json");
        store.request(&key, None, Some(&bytes), MAX_MANIFEST)?;
        if store.request(&key, None, None, MAX_MANIFEST)? != bytes {
            return Err("保存したmanifestが一致しません".into());
        }
    }
    println!(
        "R2 CI成果物の{}が完了しました: {prefix}",
        if output.is_some() {
            "取得・検証"
        } else {
            "保存・再読検証"
        }
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Read,
        net::TcpListener,
        thread,
        time::{Duration, Instant},
    };

    fn identity() -> Identity {
        Identity {
            commit: "a".repeat(40),
            run: "123".into(),
            attempt: "1".into(),
            target: "windows-x64".into(),
        }
    }
    fn fixture() -> (Identity, Vec<u8>, Vec<Vec<u8>>) {
        fixture_for("windows-x64")
    }
    fn fixture_for(target: &str) -> (Identity, Vec<u8>, Vec<Vec<u8>>) {
        let mut id = identity();
        id.target = target.into();
        let files = vec![
            b"native binary fixture".to_vec(),
            b"license fixture".to_vec(),
        ];
        let m = Manifest {
            schema: 1,
            identity: id.clone(),
            files: id
                .names()
                .unwrap()
                .into_iter()
                .zip(&files)
                .map(|(name, bytes)| Entry {
                    name: name.into(),
                    size: bytes.len() as u64,
                    sha256: digest(bytes),
                })
                .collect(),
        };
        (id, serde_json::to_vec(&m).unwrap(), files)
    }
    // 実curlを別プロセスで動かし、署名付き要求と実際の転送内容を確認する。
    fn server(
        responses: Vec<(&'static str, Vec<u8>)>,
    ) -> (Store, thread::JoinHandle<Vec<Vec<u8>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}/", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, body) in responses {
                let start = Instant::now();
                let (mut socket, _) = loop {
                    match listener.accept() {
                        Ok(socket) => break socket,
                        Err(e)
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                && start.elapsed() < Duration::from_secs(10) =>
                        {
                            thread::sleep(Duration::from_millis(10))
                        }
                        Err(e) => panic!("fixture accept: {e}"),
                    }
                };
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                loop {
                    let mut b = [0; 1];
                    socket.read_exact(&mut b).unwrap();
                    request.push(b[0]);
                    if request.ends_with(b"\r\n\r\n") {
                        break;
                    }
                }
                let headers = String::from_utf8(request.clone()).unwrap().to_lowercase();
                assert!(
                    headers.contains("authorization: aws4-hmac-sha256 credential=fake-access/")
                );
                assert!(headers.contains("/auto/s3/aws4_request"));
                assert!(!headers.contains("fake-secret"));
                if headers.contains("expect: 100-continue") {
                    socket.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").unwrap();
                }
                let length = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length: "))
                    .map(|s| s.trim().parse::<usize>().unwrap())
                    .unwrap_or(0);
                let mut payload = vec![0; length];
                socket.read_exact(&mut payload).unwrap();
                request.extend(payload);
                socket
                    .write_all(
                        format!(
                            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        )
                        .as_bytes(),
                    )
                    .unwrap();
                socket.write_all(&body).unwrap();
                requests.push(request);
            }
            requests
        });
        (
            Store {
                endpoint,
                user: "fake-access:fake-secret".into(),
            },
            handle,
        )
    }
    fn directory(name: &str) -> std::path::PathBuf {
        let temporary = env::temp_dir();
        #[cfg(unix)]
        let temporary = fs::canonicalize(temporary).unwrap();
        let root = temporary.join(format!("r2-{}-{name}", std::process::id()));
        fs::create_dir(&root).unwrap();
        root
    }
    #[test]
    fn legacy_macos_upload_is_rejected_before_store_access() {
        let (id, _, _) = fixture_for("macos-universal");
        let store = Store {
            endpoint: "http://127.0.0.1:9".into(),
            user: "fake-access:fake-secret".into(),
        };
        let error = transfer(&id, &store, None).unwrap_err().to_string();
        assert!(error.contains("読取り専用"));
    }

    #[test]
    fn all_targets_keep_two_files_and_fixed_sources() {
        for (target, binary, source_directory) in [
            ("windows-x64", "plumivox.exe", "dist"),
            ("macos-arm64", "plumivox", "dist"),
            ("macos-universal", "plumivox", "dist"),
            ("vps-linux-x86_64", "plumivox-vps-setup-x86_64", "setup/vps"),
            (
                "vps-linux-aarch64",
                "plumivox-vps-setup-aarch64",
                "setup/vps",
            ),
        ] {
            let (id, m, files) = fixture_for(target);
            id.validate().unwrap();
            assert_eq!(id.names().unwrap(), [binary, "THIRD-PARTY-LICENSES.txt"]);
            let repository = Path::new("repository");
            assert_eq!(
                id.source(repository, binary),
                repository.join(source_directory).join(binary)
            );
            assert_eq!(
                id.source(repository, "THIRD-PARTY-LICENSES.txt"),
                repository.join("dist/THIRD-PARTY-LICENSES.txt")
            );
            let output = directory(&format!("all-targets-{target}")).join("new");
            let (store, handle) = server(vec![
                ("200 OK", m.clone()),
                ("200 OK", files[0].clone()),
                ("200 OK", files[1].clone()),
            ]);
            transfer(&id, &store, Some(&output)).unwrap();
            assert_eq!(fs::read(output.join(binary)).unwrap(), files[0], "{target}");
            assert_eq!(
                fs::read(output.join("THIRD-PARTY-LICENSES.txt")).unwrap(),
                files[1],
                "{target}"
            );
            assert_eq!(
                fs::read(output.join("manifest.json")).unwrap(),
                m,
                "{target}"
            );
            #[cfg(unix)]
            if target != "windows-x64" {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    fs::metadata(output.join(binary))
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o777,
                    0o755
                );
            }
            let requests = handle.join().unwrap();
            assert_eq!(requests.len(), 3, "{target}");
            assert!(
                String::from_utf8_lossy(&requests[0])
                    .starts_with(&format!("GET /{}manifest.json ", id.prefix())),
                "{target}"
            );
        }
    }

    #[test]
    fn vps_upload_reads_fixed_binary_and_license_and_rejects_other_arch_manifest() {
        for target in ["vps-linux-x86_64", "vps-linux-aarch64"] {
            let (id, m, files) = fixture_for(target);
            let repository = directory(target);
            fs::create_dir_all(repository.join("setup/vps")).unwrap();
            fs::create_dir(repository.join("dist")).unwrap();
            for (name, data) in id.names().unwrap().iter().zip(&files) {
                fs::write(id.source(&repository, name), data).unwrap();
            }
            let (store, handle) = server(vec![
                ("200 OK", vec![]),
                ("200 OK", files[0].clone()),
                ("200 OK", vec![]),
                ("200 OK", files[1].clone()),
                ("200 OK", vec![]),
                ("200 OK", m.clone()),
            ]);
            transfer_from(&id, &store, None, &repository).unwrap();
            let requests = handle.join().unwrap();
            for index in [0, 2, 4] {
                assert!(
                    String::from_utf8_lossy(&requests[index])
                        .to_ascii_lowercase()
                        .contains("if-none-match: *")
                );
            }
            assert!(requests[4].ends_with(&m));
            let mut value: serde_json::Value = serde_json::from_slice(&m).unwrap();
            for wrong in ["../plumivox-vps-setup-x86_64", "plumivox.exe", "unexpected"] {
                value["files"][0]["name"] = wrong.into();
                assert!(manifest(&serde_json::to_vec(&value).unwrap(), &id).is_err());
            }
            let mut other = id.clone();
            other.target = if target.ends_with("x86_64") {
                "vps-linux-aarch64"
            } else {
                "vps-linux-x86_64"
            }
            .into();
            assert!(manifest(&m, &other).is_err());
            let (store, handle) = server(vec![("200 OK", m), ("200 OK", b"corrupt".to_vec())]);
            let output = repository.join("rejected");
            assert!(transfer(&id, &store, Some(&output)).is_err());
            assert!(!output.exists());
            handle.join().unwrap();
        }
    }

    #[test]
    fn native_curl_upload_is_conditional_and_preserves_bytes() {
        let path = directory("upload").join("binary with spaces");
        let data = b"\0native\xff\r\n";
        fs::write(&path, data).unwrap();
        let (store, handle) = server(vec![("200 OK", vec![]), ("200 OK", data.to_vec())]);
        store
            .request("object", Some(&path), None, MAX_MANIFEST)
            .unwrap();
        assert_eq!(store.request("object", None, None, MAX_FILE).unwrap(), data);
        let requests = handle.join().unwrap();
        let request = &requests[0];
        assert!(request.ends_with(data));
        assert!(
            String::from_utf8_lossy(request)
                .to_lowercase()
                .contains("if-none-match: *")
        );
    }
    #[test]
    fn upload_publishes_manifest_only_after_both_files_are_verified() {
        let (id, m, files) = fixture();
        let dist = directory("publish");
        fs::create_dir(dist.join("dist")).unwrap();
        for (name, data) in id.names().unwrap().iter().zip(&files) {
            fs::write(id.source(&dist, name), data).unwrap();
        }
        let (store, handle) = server(vec![
            ("200 OK", vec![]),
            ("200 OK", files[0].clone()),
            ("200 OK", vec![]),
            ("200 OK", files[1].clone()),
            ("200 OK", vec![]),
            ("200 OK", m.clone()),
        ]);
        transfer_from(&id, &store, None, &dist).unwrap();
        let requests = handle.join().unwrap();
        assert!(
            String::from_utf8_lossy(&requests[4])
                .starts_with(&format!("PUT /{}manifest.json ", id.prefix()))
        );
        assert!(requests[4].ends_with(&m));
    }
    #[test]
    fn failed_upload_does_not_publish_manifest() {
        let (id, _, files) = fixture();
        let dist = directory("failed-publish");
        fs::create_dir(dist.join("dist")).unwrap();
        for (name, data) in id.names().unwrap().iter().zip(&files) {
            fs::write(id.source(&dist, name), data).unwrap();
        }
        let (store, handle) = server(vec![("200 OK", vec![]), ("200 OK", b"bad".to_vec())]);
        assert!(transfer_from(&id, &store, None, &dist).is_err());
        let requests = handle.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(
            !requests
                .iter()
                .any(|r| String::from_utf8_lossy(r).contains("manifest.json"))
        );
    }
    #[test]
    fn corrupted_download_never_creates_output() {
        let (id, m, _) = fixture();
        let (store, handle) = server(vec![("200 OK", m), ("200 OK", b"corruption".to_vec())]);
        let output = directory("corrupt").join("new");
        assert!(transfer(&id, &store, Some(&output)).is_err());
        assert!(!output.exists());
        handle.join().unwrap();
    }
    #[test]
    fn manifest_rejects_wrong_identity_paths_duplicates_and_unknown_fields() {
        let (id, m, _) = fixture();
        let mut value: serde_json::Value = serde_json::from_slice(&m).unwrap();
        for name in [
            "../plumivox.exe",
            "/plumivox.exe",
            "THIRD-PARTY-LICENSES.txt",
        ] {
            value["files"][0]["name"] = name.into();
            assert!(manifest(&serde_json::to_vec(&value).unwrap(), &id).is_err());
        }
        let mut other = id.clone();
        other.run = "456".into();
        assert!(manifest(&m, &other).is_err());
        assert!(manifest(b"{}", &id).is_err());
        assert!(manifest(&vec![b' '; MAX_MANIFEST + 1], &id).is_err());
        value = serde_json::from_slice(&m).unwrap();
        value["extra"] = true.into();
        assert!(manifest(&serde_json::to_vec(&value).unwrap(), &id).is_err());
    }
    #[test]
    fn redirects_and_missing_manifest_are_rejected() {
        for status in ["302 Found", "404 Not Found"] {
            let (store, handle) = server(vec![(status, vec![])]);
            assert!(
                store
                    .request("manifest.json", None, None, MAX_MANIFEST)
                    .is_err()
            );
            handle.join().unwrap();
        }
    }
    #[test]
    fn config_escaping_and_identity_validation() {
        assert_eq!(quote("a\\b\"c").unwrap(), "\"a\\\\b\\\"c\"");
        assert!(quote("secret\nurl=evil").is_err());
        let mut id = identity();
        id.commit = "../evil".into();
        assert!(id.validate().is_err());
        id = identity();
        id.attempt = "0".into();
        assert!(id.validate().is_err());
    }

    #[test]
    fn output_and_files_never_replace_existing_data() {
        let parent = directory("existing");
        let output = parent.join("new");
        let (output, _leases) = create_output(&output).unwrap();
        write_artifact(&output, "plumivox.exe", b"original").unwrap();
        assert!(write_artifact(&output, "plumivox.exe", b"replacement").is_err());
        assert_eq!(fs::read(output.join("plumivox.exe")).unwrap(), b"original");
        assert!(create_output(&output).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn windows_output_and_parent_are_held_against_rename() {
        let parent = directory("directory-lease");
        let (output, leases) = create_output(&parent.join("new")).unwrap();
        assert!(fs::rename(&output, parent.join("moved")).is_err());
        assert!(fs::rename(&parent, parent.with_extension("moved")).is_err());
        drop(leases);
        fs::rename(&output, parent.join("moved")).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_shared_parent_stays_unchanged_and_artifacts_are_private() {
        let root = directory("shared-acl");
        let parent = root.join("shared");
        windows_private::shared_parent(&parent).unwrap();
        let parent_handle = lease_directory(&parent).unwrap();
        let before = windows_private::security_text(&parent_handle).unwrap();
        assert!(before.contains("WD"), "fixture must grant Everyone access");
        let (output, leases) = create_output(&parent.join("private")).unwrap();
        let output_handle = leases.last().unwrap();
        windows_private::validate(output_handle, true).unwrap();
        assert!(
            !windows_private::security_text(output_handle)
                .unwrap()
                .contains("WD")
        );
        let data = b"verified artifact bytes";
        write_artifact(&output, "plumivox.exe", data).unwrap();
        let file = fs::File::open(output.join("plumivox.exe")).unwrap();
        // 作成時の本人限定・継承保護ACLに、共有親のEveryone許可が入らないことを確認する。
        windows_private::validate(&file, true).unwrap();
        assert!(
            !windows_private::security_text(&file)
                .unwrap()
                .contains("WD")
        );
        assert_eq!(
            digest(&fs::read(output.join("plumivox.exe")).unwrap()),
            digest(data)
        );
        assert_eq!(
            windows_private::security_text(&parent_handle).unwrap(),
            before
        );
        // 拒否時も既存dirのACLを修復・変更しない。
        assert!(create_output(&parent).is_err());
        assert_eq!(
            windows_private::security_text(&parent_handle).unwrap(),
            before
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_private_directory_and_fd_permissions_reject_link_substitution() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
        let parent = directory("unix-output");
        let (output, _leases) = create_output(&parent.join("new")).unwrap();
        assert_eq!(fs::metadata(&output).unwrap().mode() & 0o777, 0o700);
        symlink(&output, parent.join("linked")).unwrap();
        assert!(create_output(&parent.join("linked/child")).is_err());
        let victim = parent.join("victim");
        fs::write(&victim, b"keep private").unwrap();
        fs::set_permissions(&victim, fs::Permissions::from_mode(0o600)).unwrap();
        let original = output.join("plumivox");
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&original)
            .unwrap();
        let moved = output.join("held-file");
        fs::rename(&original, &moved).unwrap();
        symlink(&victim, &original).unwrap();
        set_executable(&file).unwrap();
        assert_eq!(file.metadata().unwrap().mode() & 0o777, 0o755);
        assert_eq!(fs::metadata(&victim).unwrap().mode() & 0o777, 0o600);
    }

    #[test]
    fn chunked_response_is_bounded_and_curl_is_reaped() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let store = Store {
            endpoint: format!("http://{}/", listener.local_addr().unwrap()),
            user: "fake-access:fake-secret".into(),
        };
        let server = thread::spawn(move || {
            let start = Instant::now();
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            && start.elapsed() < Duration::from_secs(10) =>
                    {
                        thread::sleep(Duration::from_millis(10))
                    }
                    Err(e) => panic!("fixture accept: {e}"),
                }
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            socket
                .set_write_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut headers = Vec::new();
            while !headers.ends_with(b"\r\n\r\n") {
                let mut b = [0];
                socket.read_exact(&mut b).unwrap();
                headers.push(b[0]);
            }
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
            // 終端chunkを送らず、curl終了によって接続が閉じることを確認する。
            socket.write_all(b"10000\r\n").unwrap();
            let _ = socket.write_all(&[b'x'; 65536]);
            let _ = socket.write_all(b"\r\n");
            let mut byte = [0];
            match socket.read(&mut byte) {
                Ok(0) => (),
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
                    ) =>
                {
                    ()
                }
                other => panic!("curl connection was not closed: {other:?}"),
            }
        });
        let start = Instant::now();
        let error = store.request("chunked", None, None, 32).unwrap_err();
        assert_eq!(error.to_string(), "R2応答がサイズ上限を超えています");
        assert!(start.elapsed() < Duration::from_secs(10));
        server.join().unwrap();
    }
}
