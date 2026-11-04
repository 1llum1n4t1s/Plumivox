//! Windowsの保存先検証と本人限定ACL。所有者は変更しない。
use super::fail;
use crate::error::Result;
use sha2::{Digest, Sha256};
#[path = "setup_windows_acl.rs"]
mod acl;
#[path = "setup_windows_console.rs"]
mod console;
use std::{
    fs, io,
    os::windows::{ffi::OsStrExt, fs::MetadataExt},
    path::{Component, Path, PathBuf},
};
use std::{
    fs::{File, OpenOptions},
    io::{ErrorKind, Read},
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
};
use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS, GetLastError};
use windows_sys::Win32::Globalization::{LCMAP_LOWERCASE, LCMapStringEx, LOCALE_NAME_INVARIANT};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
    FILE_READ_ATTRIBUTES, FILE_RENAME_INFO, FILE_SHARE_READ, FileRenameInfo, READ_CONTROL,
    SetFileInformationByHandle, WRITE_DAC,
};

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
        .unwrap_or_default()
}
pub(super) fn default_key() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join("AppData/Local"))
        .join("Kagayoi/Plumivox/keys/plumivox_ed25519")
}
pub(super) fn secret(prompt: &str) -> Result<String> {
    console::secret(prompt)
}
fn canonical_with_missing_tail(path: &Path) -> Result<PathBuf> {
    // `.`を除去し、既存部分の8.3名も実際の長い名前へ解決する。
    let mut ancestor = std::path::absolute(path).map_err(|_| fail("PATH_INVALID"))?;
    let mut tail = Vec::new();
    loop {
        match fs::symlink_metadata(&ancestor) {
            Ok(_) => {
                let resolved = fs::canonicalize(&ancestor).map_err(|_| fail("PATH_INVALID"))?;
                let text = resolved.to_str().ok_or_else(|| fail("PATH_INVALID"))?;
                // canonicalizeが付けるローカルドライブ用の拡張接頭辞だけを外す。
                let text = text.strip_prefix(r"\\?\").unwrap_or(text);
                if text.starts_with("UNC\\") || text.starts_with(r"\\") {
                    return Err(fail("PATH_INVALID"));
                }
                let mut canonical = PathBuf::from(text);
                for component in tail.iter().rev() {
                    canonical.push(component);
                }
                return Ok(canonical);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                tail.push(
                    ancestor
                        .file_name()
                        .ok_or_else(|| fail("PATH_INVALID"))?
                        .to_owned(),
                );
                ancestor = ancestor
                    .parent()
                    .ok_or_else(|| fail("PATH_INVALID"))?
                    .to_owned();
            }
            Err(_) => return Err(fail("PATH_INVALID")),
        }
    }
}
pub(super) fn path_ok(path: &Path) -> Result<()> {
    // UNC、デバイスパス、ADS、親への遡行は受け付けない。
    let text = path.to_str().ok_or_else(|| fail("PATH_INVALID"))?;
    if !path.is_absolute()
        || text.starts_with(r"\\")
        || text.get(2..).is_none_or(|v| v.contains(':'))
        || path.components().any(|c| matches!(c, Component::ParentDir))
    {
        return Err(fail("PATH_INVALID"));
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(meta) if meta.file_attributes() & 0x400 != 0 => return Err(fail("PATH_INVALID")),
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return Err(fail("PATH_INVALID")),
        }
    }
    let mut blocked = vec![
        home().join(".codex"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")),
    ];
    if let Some(root) = std::env::var_os("CODEX_HOME").filter(|v| !v.is_empty()) {
        blocked.push(root.into());
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
    let candidate = canonical_with_missing_tail(path)?;
    let candidate = PathBuf::from(candidate.to_string_lossy().to_lowercase());
    for root in blocked {
        // ビルド時のリポジトリが別PCのドライブにあっても通常の保存先を拒否しない。
        // 存在する禁止先は実体を比較し、解決できない禁止先も絶対パスで比較する。
        let root = canonical_with_missing_tail(&root)
            .or_else(|_| std::path::absolute(&root).map_err(|_| fail("PATH_INVALID")))?;
        let root = PathBuf::from(root.to_string_lossy().to_lowercase());
        if candidate.starts_with(root) {
            return Err(fail("PATH_INVALID"));
        }
    }
    Ok(())
}
pub(super) fn private(path: &Path, directory: bool) -> Result<()> {
    path_ok(path)?;
    let meta = fs::symlink_metadata(path).map_err(|_| fail("ACL_UNSAFE"))?;
    if meta.is_dir() != directory || (!directory && !meta.is_file()) {
        return Err(fail("ACL_UNSAFE"));
    }
    let _parents = acl::guard_parent(path, false)?;
    acl::open(path, directory, false, true).map(|_| ())
}
#[cfg(test)]
fn directory(path: &Path) -> Result<()> {
    acl::guard_parent(&path.join(".plumivox-parent-probe"), true).map(|_| ())
}
pub(super) fn guard_parent(path: &Path, create: bool) -> Result<Vec<File>> {
    acl::guard_parent(path, create)
}
pub(super) fn protect(path: &Path) -> Result<()> {
    let _parents = acl::guard_parent(path, false)?;
    acl::open(path, false, true, false).map(|_| ())
}
pub(super) fn read_file(path: &Path, private: bool) -> Result<File> {
    path_ok(path)?;
    let file = OpenOptions::new()
        .access_mode(FILE_GENERIC_READ)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| fail("ACL_UNSAFE"))?;
    acl::verify(&file, false, private, false)?;
    Ok(file)
}
pub(super) fn snapshot(path: &Path, private: bool) -> Result<Option<Vec<u8>>> {
    path_ok(path)?;
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(fail("SETTINGS_INVALID")),
        Ok(meta) if !meta.is_file() || meta.len() > 2097152 => {
            return Err(fail("SETTINGS_INVALID"));
        }
        Ok(_) => {}
    }
    let _parents = acl::guard_parent(path, false)?;
    let file = read_file(path, private)?;
    let mut bytes = Vec::new();
    file.take(2097153)
        .read_to_end(&mut bytes)
        .map_err(|_| fail("SETTINGS_INVALID"))?;
    if bytes.len() > 2097152 {
        return Err(fail("SETTINGS_INVALID"));
    }
    Ok(Some(bytes))
}
pub(super) struct StagingFile {
    file: File,
    cleanup_allowed: bool,
}
impl std::ops::Deref for StagingFile {
    type Target = File;
    fn deref(&self) -> &File {
        &self.file
    }
}
impl std::ops::DerefMut for StagingFile {
    fn deref_mut(&mut self) -> &mut File {
        &mut self.file
    }
}
impl Drop for StagingFile {
    fn drop(&mut self) {
        if self.cleanup_allowed {
            let _ = self.finish(Err(fail("SETTINGS_INVALID")));
        }
    }
}
fn dispose_file(file: &File) -> bool {
    use windows_sys::Win32::Storage::FileSystem::{FILE_DISPOSITION_INFO, FileDispositionInfo};
    let info = FILE_DISPOSITION_INFO { DeleteFile: true };
    // パスを再解決せず、今回作成して保持中の対象だけを回収する。
    unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfo,
            (&info as *const FILE_DISPOSITION_INFO).cast(),
            std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
        ) != 0
    }
}
impl StagingFile {
    pub(super) fn finish(&mut self, result: Result<()>) -> Result<()> {
        self.finish_with_dispose(result, dispose_file)
    }
    fn finish_with_dispose(
        &mut self,
        result: Result<()>,
        dispose: impl FnOnce(&File) -> bool,
    ) -> Result<()> {
        if result.is_err() && self.cleanup_allowed {
            self.cleanup_allowed = false;
            if !dispose(&self.file) {
                return Err(fail("SETTINGS_TEMP_RETAINED"));
            }
        }
        result
    }
}
pub(super) fn staging(path: &Path) -> Result<StagingFile> {
    path_ok(path)?;
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .access_mode(FILE_GENERIC_WRITE | READ_CONTROL | WRITE_DAC | DELETE | FILE_READ_ATTRIBUTES)
        .share_mode(0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| fail("SETTINGS_INVALID"))?;
    let mut staging = StagingFile {
        file,
        cleanup_allowed: true,
    };
    if let Err(error) = acl::apply(&staging, false) {
        return Err(staging.finish(Err(error)).unwrap_err());
    }
    Ok(staging)
}
pub(super) fn verify_staging(file: &File) -> Result<()> {
    acl::verify(file, false, true, true)
}
pub(super) fn publish(
    file: &mut StagingFile,
    destination: &Path,
    replace_existing: bool,
) -> Result<()> {
    publish_with_rename(
        file,
        destination,
        replace_existing,
        |file, info, bytes| unsafe {
            let moved =
                SetFileInformationByHandle(file.as_raw_handle(), FileRenameInfo, info, bytes);
            (moved, GetLastError())
        },
    )
}
fn publish_with_rename(
    file: &mut StagingFile,
    destination: &Path,
    replace_existing: bool,
    rename: impl FnOnce(&File, *const core::ffi::c_void, u32) -> (i32, u32),
) -> Result<()> {
    publish_with_operations(file, destination, replace_existing, rename, handle_name)
}
fn handle_name(file: &File) -> Option<Vec<u16>> {
    use windows_sys::Win32::Storage::FileSystem::GetFinalPathNameByHandleW;
    unsafe {
        let needed = GetFinalPathNameByHandleW(file.as_raw_handle(), std::ptr::null_mut(), 0, 0);
        if needed == 0 || needed > 65536 {
            return None;
        }
        let mut name = vec![0u16; needed as usize];
        let written = GetFinalPathNameByHandleW(file.as_raw_handle(), name.as_mut_ptr(), needed, 0);
        if written == 0 || written >= needed {
            return None;
        }
        name.truncate(written as usize);
        Some(name)
    }
}
fn publish_with_operations(
    file: &mut StagingFile,
    destination: &Path,
    replace_existing: bool,
    rename: impl FnOnce(&File, *const core::ffi::c_void, u32) -> (i32, u32),
    mut inspect_name: impl FnMut(&File) -> Option<Vec<u16>>,
) -> Result<()> {
    path_ok(destination)?;
    verify_staging(file)?;
    // Win32のFileNameはNUL終端し、FileNameLengthには終端を含めない。
    let name = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
    let bytes = offset
        .checked_add(
            name.len()
                .checked_mul(2)
                .ok_or_else(|| fail("PATH_INVALID"))?,
        )
        .ok_or_else(|| fail("PATH_INVALID"))?
        .max(std::mem::size_of::<FILE_RENAME_INFO>());
    let mut buffer = vec![0usize; bytes.div_ceil(std::mem::size_of::<usize>())];
    let bytes = u32::try_from(bytes).map_err(|_| fail("PATH_INVALID"))?;
    unsafe {
        let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
        (*info).Anonymous.ReplaceIfExists = replace_existing;
        (*info).RootDirectory = std::ptr::null_mut();
        (*info).FileNameLength =
            u32::try_from((name.len() - 1) * 2).map_err(|_| fail("PATH_INVALID"))?;
        std::ptr::copy_nonoverlapping(name.as_ptr(), (*info).FileName.as_mut_ptr(), name.len());
    }
    let original_name = inspect_name(file);
    // 要求後は移動済みの可能性がある。応答の成否にかかわらず、このhandleの
    // 自動削除を止め、正本になった実体をDropで消さない。
    file.cleanup_allowed = false;
    let result = rename(file, buffer.as_ptr().cast(), bytes);
    if result.0 == 0 {
        if !replace_existing
            && matches!(result.1, ERROR_ALREADY_EXISTS | ERROR_FILE_EXISTS)
            && original_name
                .as_ref()
                .is_some_and(|original| inspect_name(file).as_ref() == Some(original))
        {
            // 同じhandleが要求前の正規化した一時名に留まると確認できた場合だけ回収する。
            file.cleanup_allowed = true;
            return file.finish(Err(fail("SETTINGS_CHANGED")));
        }
        return Err(fail("SETTINGS_SAVE_UNCERTAIN"));
    }
    file.sync_all().map_err(|_| fail("SETTINGS_SAVE_UNCERTAIN"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_mutex_casing_matches_dotnet_invariant_not_full_unicode_lowercase() {
        assert_eq!(
            invariant_lower(r"C:\ASCII\SETTINGS.JSON").unwrap(),
            r"c:\ascii\settings.json"
        );
        // .NET invariantはU+0130を保持し、文脈に依存せずΣをσへ変換する。
        assert_eq!(
            invariant_lower(r"C:\İ\ΟΣ\Ä Settings.JSON").unwrap(),
            r"c:\İ\οσ\ä settings.json"
        );
    }

    #[test]
    fn initial_publish_preserves_concurrent_destination_and_publishes_when_absent() {
        let root = std::env::temp_dir().join(format!(
            "plumivox-setup-publish-{:032x}",
            rand::random::<u128>()
        ));
        directory(&root).unwrap();
        struct Area(PathBuf);
        impl Drop for Area {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let area = Area(root);
        let staging = area.0.join("staging.json");
        let destination = area.0.join("settings.json");
        let _parents = guard_parent(&staging, false).unwrap();
        let mut source = super::staging(&staging).unwrap();
        use std::io::Write;
        source.write_all(b"new settings").unwrap();
        fs::write(&destination, b"concurrent settings").unwrap();
        assert_eq!(
            publish(&mut source, &destination, false).unwrap_err().code,
            "SETTINGS_CHANGED"
        );
        assert_eq!(fs::read(&destination).unwrap(), b"concurrent settings");
        drop(source);
        assert!(!staging.exists());
        let mut source = super::staging(&staging).unwrap();
        source.write_all(b"new settings").unwrap();
        // 排他handleを保持しているため他のプロセスは一時名を差し替えられない。
        assert!(fs::rename(&staging, area.0.join("stolen.json")).is_err());
        let absent = area.0.join("absent.json");
        publish(&mut source, &absent, false).unwrap();
        drop(source);
        assert_eq!(fs::read(absent).unwrap(), b"new settings");
        assert!(!staging.exists());

        // ファイルシステムでは置換済みだが呼出し側には失敗が返った境界を再現する。
        let uncertain_path = area.0.join("uncertain-staging.json");
        let mut uncertain = super::staging(&uncertain_path).unwrap();
        uncertain.write_all(b"replacement settings").unwrap();
        let result = publish_with_rename(
            &mut uncertain,
            &destination,
            true,
            |file, info, bytes| unsafe {
                assert_ne!(
                    SetFileInformationByHandle(file.as_raw_handle(), FileRenameInfo, info, bytes),
                    0
                );
                (0, windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED)
            },
        );
        assert_eq!(result.unwrap_err().code, "SETTINGS_SAVE_UNCERTAIN");
        drop(uncertain);
        assert_eq!(fs::read(&destination).unwrap(), b"replacement settings");
        assert!(!uncertain_path.exists());

        // 確定した初回衝突では元の一時実体だけを回収し、既存正本を保持する。
        let collision_path = area.0.join("collision-staging.json");
        let mut collision = super::staging(&collision_path).unwrap();
        collision.write_all(b"unpublished settings").unwrap();
        assert_eq!(
            publish(&mut collision, &destination, false)
                .unwrap_err()
                .code,
            "SETTINGS_CHANGED"
        );
        drop(collision);
        assert!(!collision_path.exists());
        assert_eq!(fs::read(&destination).unwrap(), b"replacement settings");

        // 衝突でも要求後の一時名を確認できなければ、勝手に削除しない。
        let unknown_path = area.0.join("unknown-staging.json");
        let mut unknown = super::staging(&unknown_path).unwrap();
        unknown.write_all(b"retained settings").unwrap();
        let mut inspections = 0;
        let result = publish_with_operations(
            &mut unknown,
            &destination,
            false,
            |file, info, bytes| unsafe {
                let moved =
                    SetFileInformationByHandle(file.as_raw_handle(), FileRenameInfo, info, bytes);
                (moved, GetLastError())
            },
            |file| {
                inspections += 1;
                if inspections == 1 {
                    handle_name(file)
                } else {
                    None
                }
            },
        );
        assert_eq!(result.unwrap_err().code, "SETTINGS_SAVE_UNCERTAIN");
        drop(unknown);
        assert_eq!(fs::read(&unknown_path).unwrap(), b"retained settings");
        assert_eq!(fs::read(&destination).unwrap(), b"replacement settings");

        // 移動済みなのにEXISTS応答となった場合も、一時名の変化で正本を保護する。
        let moved_path = area.0.join("moved-staging.json");
        let moved_destination = area.0.join("moved-settings.json");
        let mut moved = super::staging(&moved_path).unwrap();
        moved.write_all(b"published despite error").unwrap();
        let result = publish_with_rename(
            &mut moved,
            &moved_destination,
            false,
            |file, info, bytes| unsafe {
                assert_ne!(
                    SetFileInformationByHandle(file.as_raw_handle(), FileRenameInfo, info, bytes),
                    0
                );
                (0, ERROR_ALREADY_EXISTS)
            },
        );
        assert_eq!(result.unwrap_err().code, "SETTINGS_SAVE_UNCERTAIN");
        drop(moved);
        assert_eq!(
            fs::read(moved_destination).unwrap(),
            b"published despite error"
        );
        assert!(!moved_path.exists());

        // 公開前のエラーと確定衝突で回収APIが失敗した場合は残存を明示する。
        for (name, original_error) in [
            ("write-failed.json", "SETTINGS_INVALID"),
            ("collision-cleanup-failed.json", "SETTINGS_CHANGED"),
        ] {
            let retained_path = area.0.join(name);
            let mut retained = super::staging(&retained_path).unwrap();
            retained.write_all(b"retained on cleanup failure").unwrap();
            let result = retained.finish_with_dispose(Err(fail(original_error)), |_| false);
            assert_eq!(result.unwrap_err().code, "SETTINGS_TEMP_RETAINED");
            drop(retained);
            assert_eq!(
                fs::read(retained_path).unwrap(),
                b"retained on cleanup failure"
            );
        }
        // FILE_RENAME_INFOの終端が割当て丸めのゼロ余白に依存しないよう全4境界を通す。
        for length in 1..=4 {
            let temporary = area.0.join(format!("boundary-staging-{length}"));
            let target = area.0.join(format!("published-{}", "x".repeat(length)));
            fs::write(&target, b"old boundary settings").unwrap();
            let mut file = super::staging(&temporary).unwrap();
            file.write_all(b"new boundary settings").unwrap();
            publish(&mut file, &target, true).unwrap();
            drop(file);
            assert_eq!(fs::read(&target).unwrap(), b"new boundary settings");
            assert!(!temporary.exists());
        }
    }
}

fn invariant_lower(value: &str) -> Result<String> {
    let source = value.encode_utf16().collect::<Vec<_>>();
    let length = i32::try_from(source.len()).map_err(|_| fail("PATH_INVALID"))?;
    let mut destination = vec![0u16; source.len()];
    // .NET TextInfo.NlsChangeCaseのInvariantと同じく、言語固有の変換を指定しない。
    let written = unsafe {
        LCMapStringEx(
            LOCALE_NAME_INVARIANT,
            LCMAP_LOWERCASE,
            source.as_ptr(),
            length,
            destination.as_mut_ptr(),
            length,
            std::ptr::null(),
            std::ptr::null(),
            0,
        )
    };
    if written != length {
        return Err(fail("PATH_INVALID"));
    }
    String::from_utf16(&destination).map_err(|_| fail("PATH_INVALID"))
}

// Mutexは専用スレッドだけが取得・解放し、asyncの継続先が変わっても所有権を保つ。
pub(super) struct SetupLock {
    release: Option<std::sync::mpsc::Sender<()>>,
    owner: Option<std::thread::JoinHandle<()>>,
}
impl Drop for SetupLock {
    fn drop(&mut self) {
        self.release.take();
        if let Some(owner) = self.owner.take() {
            let _ = owner.join();
        }
    }
}
pub(super) fn lock(path: &Path) -> Result<SetupLock> {
    path_ok(path)?;
    let path = std::path::absolute(path).map_err(|_| fail("PATH_INVALID"))?;
    // 旧PS互換の字句パスと、新Rust同士の別名排他用の実パスを両方取得する。
    let canonical = canonical_with_missing_tail(&path)?;
    let mut digests = Vec::new();
    for candidate in [&path, &canonical] {
        let lower = invariant_lower(&candidate.to_string_lossy().replace('/', "\\"))?;
        let digest = hex::encode_upper(Sha256::digest(lower.as_bytes()));
        if !digests.contains(&digest) {
            digests.push(digest);
        }
    }
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    let (release, release_rx) = std::sync::mpsc::channel();
    let owner = std::thread::Builder::new()
        .name("plumivox-setup-lock".into())
        .spawn(move || {
            use std::ptr::{null, null_mut};
            use windows_sys::Win32::{
                Foundation::{CloseHandle, LocalFree, WAIT_ABANDONED, WAIT_OBJECT_0},
                Security::{
                    Authorization::ConvertSidToStringSidW, GetTokenInformation, TOKEN_QUERY,
                    TOKEN_USER, TokenUser,
                },
                System::Threading::{
                    CreateMutexW, GetCurrentProcess, OpenProcessToken, ReleaseMutex,
                    WaitForSingleObject,
                },
            };
            // Handleはこのスレッドから外へ出さず、すべての終了経路で解放する。
            unsafe {
                let mut token = null_mut();
                if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                    let _ = ready_tx.send(false);
                    return;
                }
                let mut size = 0;
                GetTokenInformation(token, TokenUser, null_mut(), 0, &mut size);
                if size == 0 || size > 65536 {
                    CloseHandle(token);
                    let _ = ready_tx.send(false);
                    return;
                }
                let mut user = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
                if GetTokenInformation(token, TokenUser, user.as_mut_ptr().cast(), size, &mut size)
                    == 0
                {
                    CloseHandle(token);
                    let _ = ready_tx.send(false);
                    return;
                }
                CloseHandle(token);
                let mut sid_text = null_mut();
                if ConvertSidToStringSidW(
                    (*(user.as_ptr().cast::<TOKEN_USER>())).User.Sid,
                    &mut sid_text,
                ) == 0
                {
                    let _ = ready_tx.send(false);
                    return;
                }
                let mut len = 0;
                while *sid_text.add(len) != 0 {
                    len += 1;
                }
                let sid = String::from_utf16_lossy(std::slice::from_raw_parts(sid_text, len));
                LocalFree(sid_text.cast());
                let mut owned = Vec::new();
                let mut acquired = true;
                for digest in digests {
                    let name = format!(r"Local\Plumivox.Setup.{sid}.{digest}")
                        .encode_utf16()
                        .chain(Some(0))
                        .collect::<Vec<_>>();
                    let mutex = CreateMutexW(null(), 0, name.as_ptr());
                    if mutex.is_null() {
                        acquired = false;
                        break;
                    }
                    // 常に非ブロッキングとし、異なる順序の取得でも待ち合わない。
                    if matches!(
                        WaitForSingleObject(mutex, 0),
                        WAIT_OBJECT_0 | WAIT_ABANDONED
                    ) {
                        owned.push(mutex);
                    } else {
                        CloseHandle(mutex);
                        acquired = false;
                        break;
                    }
                }
                let notified = ready_tx.send(acquired).is_ok();
                if acquired && notified {
                    let _ = release_rx.recv();
                }
                for mutex in owned.into_iter().rev() {
                    ReleaseMutex(mutex);
                    CloseHandle(mutex);
                }
            }
        })
        .map_err(|_| fail("SETUP_BUSY"))?;
    if ready_rx.recv().unwrap_or(false) {
        Ok(SetupLock {
            release: Some(release),
            owner: Some(owner),
        })
    } else {
        let _ = owner.join();
        Err(fail("SETUP_BUSY"))
    }
}
