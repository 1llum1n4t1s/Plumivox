//! Windows の現在の利用者SIDを取得・文字列化する共通処理。
//!
//! セッション保護（session.rs）、セットアップの ACL（setup_windows_acl.rs）、
//! セットアップの排他（setup_windows.rs）で同じ Win32 手順を重複させないために置く。
//! ここではエラーコードを持たず `Option` で失敗だけを返し、
//! `SESSION_STORAGE_UNAVAILABLE`・`ACL_UNSAFE`・`SETUP_BUSY` への対応付けは呼び出し側が行う。
//! DACL の適用・検証は用途ごとに条件が異なるため、ここへは統合しない。
use std::ptr::null_mut;
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, LocalFree},
    Security::{
        Authorization::ConvertSidToStringSidW, GetTokenInformation, PSID, TOKEN_QUERY, TOKEN_USER,
        TokenUser,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

/// TokenUser 情報の上限。異常な応答で大きな領域を確保しない。
const MAX_TOKEN_USER: u32 = 65536;

/// `LocalAlloc` 系で確保された領域を、すべての終了経路で `LocalFree` する。
pub(crate) struct Local(pub(crate) *mut core::ffi::c_void);
impl Drop for Local {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}

/// プロセストークンを、すべての終了経路で閉じる。
struct Token(HANDLE);
impl Drop for Token {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

/// 現在のプロセス利用者の TOKEN_USER をバッファごと所有する。
/// `sid()` が返すポインターはこのバッファ内を指すため、この値より長く使ってはいけない。
pub(crate) struct CurrentUser {
    // TOKEN_USER の整列を満たすため usize 単位で確保する。
    buffer: Vec<usize>,
}

impl CurrentUser {
    /// 現在のプロセストークンから利用者情報を読む。失敗時は None。
    pub(crate) fn query() -> Option<Self> {
        unsafe {
            let mut token = null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return None;
            }
            let token = Token(token);
            let mut size = 0;
            GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut size);
            if size == 0 || size > MAX_TOKEN_USER {
                return None;
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
                return None;
            }
            Some(Self { buffer })
        }
    }

    /// 利用者SID。戻り値は `self` のバッファを指すため、`self` の生存中だけ有効。
    pub(crate) fn sid(&self) -> PSID {
        unsafe { (*(self.buffer.as_ptr().cast::<TOKEN_USER>())).User.Sid }
    }

    /// 利用者SIDの文字列表現（`S-1-5-...`）を終端なしの UTF-16 で返す。失敗時は None。
    /// UTF-16 から String への変換（厳格／置換）は用途ごとの従来挙動に合わせて呼び出し側で行う。
    pub(crate) fn sid_utf16(&self) -> Option<Vec<u16>> {
        unsafe {
            let mut sid_text = null_mut();
            if ConvertSidToStringSidW(self.sid(), &mut sid_text) == 0 {
                return None;
            }
            let _sid_text = Local(sid_text.cast());
            let mut len = 0;
            while *sid_text.add(len) != 0 {
                len += 1;
            }
            Some(std::slice::from_raw_parts(sid_text, len).to_vec())
        }
    }
}
