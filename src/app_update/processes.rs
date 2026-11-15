//! 導入先から動いている、自分以外のプロセスの検出。
//!
//! Velopack の適用（`Update.exe apply`）は、導入先から起動している自分以外の全プロセスを強制終了する。
//! 別セッションの stdio MCP や、MCP が `current_exe()` から起動した共有中継 daemon（`--proxy-daemon`）を
//! 巻き込まないよう、1つでも見つかったら適用しない（強制終了はしない。次のログオンに回す）。
//! 判定は実行ファイルのパスが導入先（Velopack の root。`current\` と `Update.exe` を含む）の配下かどうかで行う。
use std::path::Path;
#[cfg(any(windows, target_os = "macos"))]
use std::path::PathBuf;

/// 比較用の正規化（拡張パスの接頭辞を外し、区切りを `/`、小文字にする。Windows の導入先は大小を区別しない。
/// macOS の既定の APFS も大小を区別しないため、同じ正規化で安全側（多めに数える側）へ倒れる）。
fn normalize(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    text.strip_prefix("//?/")
        .unwrap_or(&text)
        .to_ascii_lowercase()
}

/// `candidate` が `root` の配下か（`root` 自身は含めない。`/a/b` と `/a/bc` を取り違えない）。
pub fn is_under(root: &Path, candidate: &Path) -> bool {
    let mut root = normalize(root);
    if !root.ends_with('/') {
        root.push('/');
    }
    normalize(candidate).starts_with(&root)
}

/// 導入先 `root` の配下の実行ファイルから動いている、`own_pid` 以外のプロセスの ID。
/// 個々のプロセスの読み取りの失敗（他の利用者のプロセスなど）は読み飛ばす。列挙全体が失敗したら Err
/// （呼び出し元は適用しない）。
pub fn others_in(root: &Path, own_pid: u32) -> std::io::Result<Vec<u32>> {
    #[cfg(any(windows, target_os = "macos"))]
    {
        // macOS の proc_pidpath はリンクを解決した実体のパスを返す。導入先のパスにリンク（/var → /private/var
        // など）が含まれていると前方一致が外れ、「誰も動いていない」と誤って適用してしまうため、実体へ揃える。
        #[cfg(target_os = "macos")]
        let canonical = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        #[cfg(target_os = "macos")]
        let root = canonical.as_path();
        Ok(list_processes()?
            .into_iter()
            .filter(|(pid, image)| *pid != own_pid && is_under(root, image))
            .map(|(pid, _)| pid)
            .collect())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (root, own_pid);
        Err(std::io::Error::other(
            "process listing is not supported on this OS",
        ))
    }
}

#[cfg(windows)]
fn list_processes() -> std::io::Result<Vec<(u32, PathBuf)>> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            Threading::{
                OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
            },
        },
    };
    // SAFETY: Win32 の列挙 API。構造体は dwSize を設定して渡し、ハンドルは必ず閉じる。
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error());
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = Vec::new();
        let mut more = Process32FirstW(snapshot, &mut entry) != 0;
        while more {
            let pid = entry.th32ProcessID;
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if !handle.is_null() {
                let mut buffer = vec![0u16; 32 * 1024];
                let mut length = buffer.len() as u32;
                if QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length) != 0 {
                    found.push((
                        pid,
                        PathBuf::from(OsString::from_wide(&buffer[..length as usize])),
                    ));
                }
                CloseHandle(handle);
            }
            more = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
        Ok(found)
    }
}

/// macOS は libproc（libSystem に含まれる）で全プロセスの ID と実行ファイルのパスを得る。
/// `ps` の出力解析と違い、空白を含むパスや表示幅の切り詰めに左右されない。
#[cfg(target_os = "macos")]
fn list_processes() -> std::io::Result<Vec<(u32, PathBuf)>> {
    use std::{ffi::OsStr, os::raw::c_int, os::unix::ffi::OsStrExt};
    unsafe extern "C" {
        fn proc_listallpids(buffer: *mut c_int, buffersize: c_int) -> c_int;
        fn proc_pidpath(pid: c_int, buffer: *mut u8, buffersize: u32) -> c_int;
    }
    /// <sys/proc_info.h> の PROC_PIDPATHINFO_MAXSIZE（4 * MAXPATHLEN）。
    const PATH_MAX_SIZE: usize = 4 * 1024;
    // 数の問い合わせと列挙の間にプロセスが増えても取りこぼさないよう、余裕を持って確保し、
    // 埋まりきったら大きくして取り直す。
    // SAFETY: バッファを null にすると、必要な件数だけが返る。
    let mut capacity = (unsafe { proc_listallpids(std::ptr::null_mut(), 0) }).max(256) as usize * 2;
    let pids = loop {
        let mut buffer = vec![0 as c_int; capacity];
        // SAFETY: buffer は capacity 個の c_int を書き込める領域で、バイト数を渡す。
        let count = unsafe {
            proc_listallpids(
                buffer.as_mut_ptr(),
                (capacity * std::mem::size_of::<c_int>()) as c_int,
            )
        };
        if count < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let count = count as usize;
        if count < capacity {
            buffer.truncate(count);
            break buffer;
        }
        capacity *= 2;
    };
    let mut found = Vec::with_capacity(pids.len());
    let mut path = vec![0u8; PATH_MAX_SIZE];
    for pid in pids.into_iter().filter(|pid| *pid > 0) {
        // SAFETY: path は PATH_MAX_SIZE バイトの書き込み可能な領域。戻り値は書いたバイト数（0 以下は失敗）。
        let length = unsafe { proc_pidpath(pid, path.as_mut_ptr(), PATH_MAX_SIZE as u32) };
        if length > 0 {
            found.push((
                pid as u32,
                PathBuf::from(OsStr::from_bytes(&path[..length as usize])),
            ));
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_matching_is_directory_aware_and_case_insensitive() {
        let root = Path::new(r"C:\Users\a\AppData\Local\PlumivoxApp");
        assert!(is_under(
            root,
            Path::new(r"C:\Users\a\AppData\Local\PlumivoxApp\current\plumivox.exe")
        ));
        assert!(is_under(
            root,
            Path::new(r"c:\users\A\appdata\local\plumivoxapp\Update.exe")
        ));
        assert!(is_under(
            root,
            Path::new(r"\\?\C:\Users\a\AppData\Local\PlumivoxApp\current\x.exe")
        ));
        // 名前が前方一致するだけの別フォルダーと、導入先そのものは対象外。
        assert!(!is_under(
            root,
            Path::new(r"C:\Users\a\AppData\Local\PlumivoxApp2\x.exe")
        ));
        assert!(!is_under(root, root));
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn own_process_is_excluded_and_unrelated_roots_are_empty() {
        let exe = std::env::current_exe().unwrap();
        let root = exe.parent().unwrap();
        // 自分自身は検出に含まれない（同じフォルダーに他のプロセスが居る保証は無い）。
        let others = others_in(root, std::process::id()).unwrap();
        assert!(!others.contains(&std::process::id()));
        assert!(
            others_in(Path::new("/definitely/not/a/running/root"), 0)
                .unwrap()
                .is_empty()
        );
    }

    /// 導入先の配下から動いている別プロセス（MCP や daemon の代わり）を検出する。
    #[cfg(windows)]
    #[test]
    fn a_running_process_inside_the_root_is_detected() {
        let base = crate::app_update::tests::scratch("procs");
        let bin = base.join("current");
        std::fs::create_dir_all(&bin).unwrap();
        // System32 の ping を導入先へ複写して、そこから起動する（標準入出力は使わない）。
        let system = std::env::var_os("SystemRoot").unwrap();
        let ping = Path::new(&system).join("System32").join("PING.EXE");
        let copy = bin.join("ping.exe");
        std::fs::copy(&ping, &copy).unwrap();
        use std::os::windows::process::CommandExt;
        // テストの子プロセスでもコンソール窓を出さない（CREATE_NO_WINDOW）。
        let mut child = std::process::Command::new(&copy)
            .creation_flags(0x0800_0000)
            .args(["-n", "30", "127.0.0.1"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let found = (0..50).find_map(|_| {
            let pids = others_in(&base, std::process::id()).unwrap();
            if pids.contains(&child.id()) {
                Some(pids)
            } else {
                std::thread::sleep(std::time::Duration::from_millis(100));
                None
            }
        });
        let _ = child.kill();
        let _ = child.wait();
        assert!(found.is_some(), "the child process was not detected");
        // 自分の PID として渡したものは除外される。
        assert!(!others_in(&base, child.id()).unwrap().contains(&child.id()));
        let _ = std::fs::remove_dir_all(&base);
    }
}
