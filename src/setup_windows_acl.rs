//! セットアップ専用。検査・DACL変更は開いた同じ対象にだけ行う。
use super::{fail, path_ok};
use crate::error::Result;
use crate::windows_identity::{CurrentUser, Local};
use std::{
    fs::{File, OpenOptions},
    os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::Path,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::HANDLE,
    Security::{Authorization::*, *},
    Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ, FILE_SHARE_WRITE, READ_CONTROL, WRITE_DAC,
    },
};

/// 現在の利用者情報を読む。失敗は ACL を安全に扱えないものとして `ACL_UNSAFE` にする。
fn user() -> Result<CurrentUser> {
    CurrentUser::query().ok_or_else(|| fail("ACL_UNSAFE"))
}

fn identity(file: &File, directory: bool) -> Result<()> {
    let meta = file.metadata().map_err(|_| fail("ACL_UNSAFE"))?;
    if meta.file_attributes() & 0x400 != 0
        || meta.is_dir() != directory
        || (!directory && !meta.is_file())
    {
        return Err(fail("ACL_UNSAFE"));
    }
    Ok(())
}

pub(super) fn verify(file: &File, directory: bool, private: bool, strict: bool) -> Result<()> {
    identity(file, directory)?;
    // `user` は検証の終わりまで保持し、`sid` がバッファより長生きしないようにする。
    let user = user()?;
    let sid = user.sid();
    unsafe {
        let mut owner = null_mut();
        let mut dacl = null_mut();
        let mut descriptor = null_mut();
        if GetSecurityInfo(
            file.as_raw_handle() as HANDLE,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut descriptor,
        ) != 0
        {
            return Err(fail("ACL_UNSAFE"));
        }
        let _descriptor = Local(descriptor);
        if owner.is_null() || EqualSid(owner, sid) == 0 {
            return Err(fail("ACL_UNSAFE"));
        }
        if !private {
            return Ok(());
        }
        if dacl.is_null() {
            return Err(fail("ACL_UNSAFE"));
        }
        if strict {
            let mut control = 0;
            let mut revision = 0;
            if GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) == 0
                || control & SE_DACL_PROTECTED == 0
                || (*dacl).AceCount != 1
            {
                return Err(fail("ACL_UNSAFE"));
            }
        }
        for index in 0..(*dacl).AceCount {
            let mut ace = null_mut();
            if GetAce(dacl, index as u32, &mut ace) == 0 {
                return Err(fail("ACL_UNSAFE"));
            }
            let allowed = &*(ace.cast::<ACCESS_ALLOWED_ACE>());
            if !strict && allowed.Header.AceType == 1 {
                continue;
            }
            let allowed_sid = (&allowed.SidStart as *const u32).cast_mut().cast();
            if allowed.Header.AceType != 0
                || EqualSid(allowed_sid, sid) == 0
                || (strict && allowed.Mask & 0x1f01ff != 0x1f01ff)
            {
                return Err(fail("ACL_UNSAFE"));
            }
        }
    }
    Ok(())
}

pub(super) fn apply(file: &File, directory: bool) -> Result<()> {
    verify(file, directory, false, false)?;
    let user = user()?;
    unsafe {
        let sid_utf16 = user.sid_utf16().ok_or_else(|| fail("ACL_UNSAFE"))?;
        let sid = String::from_utf16(&sid_utf16).map_err(|_| fail("ACL_UNSAFE"))?;
        let inheritance = if directory { "OICI" } else { "" };
        let sddl = format!("D:P(A;{inheritance};FA;;;{sid})")
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
            return Err(fail("ACL_UNSAFE"));
        }
        let _descriptor = Local(descriptor);
        let mut present = 0;
        let mut defaulted = 0;
        let mut dacl = null_mut();
        if GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted) == 0
            || present == 0
            || dacl.is_null()
        {
            return Err(fail("ACL_UNSAFE"));
        }
        if SetSecurityInfo(
            file.as_raw_handle() as HANDLE,
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            dacl,
            null(),
        ) != 0
        {
            return Err(fail("ACL_UNSAFE"));
        }
    }
    verify(file, directory, true, true)
}

pub(super) fn open(path: &Path, directory: bool, apply_acl: bool, private: bool) -> Result<File> {
    path_ok(path)?;
    let mut options = OpenOptions::new();
    options
        .access_mode(READ_CONTROL | FILE_READ_ATTRIBUTES | if apply_acl { WRITE_DAC } else { 0 })
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(
            FILE_FLAG_OPEN_REPARSE_POINT
                | if directory {
                    FILE_FLAG_BACKUP_SEMANTICS
                } else {
                    0
                },
        );
    let file = options.open(path).map_err(|_| fail("ACL_UNSAFE"))?;
    if apply_acl {
        apply(&file, directory)?;
    } else {
        verify(&file, directory, private, false)?;
    }
    Ok(file)
}

// 祖先を上から順に削除共有なしで保持し、後続のパス操作中の改名・差替えを防ぐ。
pub(super) fn guard_parent(path: &Path, create: bool) -> Result<Vec<File>> {
    path_ok(path)?;
    let parent = path.parent().ok_or_else(|| fail("PATH_INVALID"))?;
    let mut ancestors = parent.ancestors().collect::<Vec<_>>();
    ancestors.reverse();
    let mut guards = Vec::new();
    for ancestor in ancestors {
        let created = match std::fs::symlink_metadata(ancestor) {
            Ok(_) => false,
            Err(error) if create && error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(ancestor).map_err(|_| fail("ACL_UNSAFE"))?;
                true
            }
            Err(_) => return Err(fail("ACL_UNSAFE")),
        };
        // 既存の祖先は所有者を変更せず保持する。所有者の検査は対象ファイル・
        // 鍵フォルダー自身と、ACLを設定する新規作成分で行う。
        let mut options = OpenOptions::new();
        options
            .access_mode(READ_CONTROL | FILE_READ_ATTRIBUTES | if created { WRITE_DAC } else { 0 })
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS);
        let file = options.open(ancestor).map_err(|_| fail("ACL_UNSAFE"))?;
        identity(&file, true)?;
        if created {
            apply(&file, true)?;
        }
        guards.push(file);
    }
    Ok(guards)
}
