//! 時間制限付きの子プロセス実行（Claude Code・Codex の CLI 呼び出しに使う）。
//!
//! Velopack はインストール・アンインストール後のフックを30秒、更新後のフックを15秒で打ち切る。
//! CLI（npm の `claude` は node を子に起動する）が固まってもフック全体を止めないよう、1回ごとに上限を設け、
//! 上限を超えたら子孫ごと止める。
//! - Windows は実行開始前（CREATE_SUSPENDED）に Job へ割り当て、`KILL_ON_JOB_CLOSE` で子孫全体を同じ寿命へ
//!   束縛する（Job のハンドルを閉じると全て止まる）。コンソール窓は出さない（CREATE_NO_WINDOW）。
//! - Unix は独立した process group を作って止める。setsid/setpgid で離脱した子孫までは保証しない。
use std::{
    io::Read,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

/// 子プロセスの失敗の種類。呼び出し元は「終了コードが非 0」だけを通常の結果として扱い、他は失敗とする。
#[derive(Debug, PartialEq, Eq)]
pub enum RunError {
    /// 起動できなかった。
    Spawn,
    /// Job への割り当てなど、子孫の束縛に失敗した（子は止めてある）。
    Job,
    /// 終了コードが 0 以外だった。
    Failed,
    /// 上限時間を超えた（子孫ごと止めた）。
    Timeout,
    /// 出力が上限を超えた（子孫ごと止めた）。
    OutputTooLarge,
    /// 出力を読めなかった。
    Pipe,
}

#[cfg(windows)]
struct Tree {
    _job: std::os::windows::io::OwnedHandle,
}
#[cfg(unix)]
struct Tree {
    pid: u32,
}

impl Tree {
    fn prepare(command: &mut Command) {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW | CREATE_SUSPENDED。Job へ入れてから主スレッドを再開する。
            command.creation_flags(0x0800_0004);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
    }

    fn attach(child: &mut std::process::Child) -> Result<Self, RunError> {
        #[cfg(windows)]
        {
            use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
            use windows_sys::Win32::{
                Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
                System::{
                    Diagnostics::ToolHelp::{
                        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First,
                        Thread32Next,
                    },
                    JobObjects::{
                        AssignProcessToJobObject, CreateJobObjectW,
                        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                        JobObjectExtendedLimitInformation, SetInformationJobObject,
                    },
                    Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME},
                },
            };
            // SAFETY: Win32 の Job・スレッド列挙 API。得たハンドルは OwnedHandle か CloseHandle で必ず閉じる。
            let result = (|| unsafe {
                let raw = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if raw.is_null() {
                    return Err(RunError::Job);
                }
                let job = OwnedHandle::from_raw_handle(raw);
                let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                if SetInformationJobObject(
                    raw,
                    JobObjectExtendedLimitInformation,
                    (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    std::mem::size_of_val(&limits) as u32,
                ) == 0
                    || AssignProcessToJobObject(raw, child.as_raw_handle()) == 0
                {
                    return Err(RunError::Job);
                }
                let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
                if snapshot == INVALID_HANDLE_VALUE {
                    return Err(RunError::Job);
                }
                let snapshot = OwnedHandle::from_raw_handle(snapshot);
                let mut entry: THREADENTRY32 = std::mem::zeroed();
                entry.dwSize = std::mem::size_of_val(&entry) as u32;
                let mut found = false;
                let mut next = Thread32First(snapshot.as_raw_handle(), &mut entry);
                while next != 0 {
                    if entry.th32OwnerProcessID == child.id() {
                        let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                        if thread.is_null() {
                            return Err(RunError::Job);
                        }
                        let resumed = ResumeThread(thread);
                        CloseHandle(thread);
                        if resumed == u32::MAX {
                            return Err(RunError::Job);
                        }
                        found = true;
                        break;
                    }
                    next = Thread32Next(snapshot.as_raw_handle(), &mut entry);
                }
                if !found {
                    return Err(RunError::Job);
                }
                Ok(Self { _job: job })
            })();
            if result.is_err() {
                let _ = child.kill();
                let _ = child.wait();
            }
            result
        }
        #[cfg(unix)]
        {
            Ok(Self { pid: child.id() })
        }
    }
}

#[cfg(unix)]
impl Drop for Tree {
    fn drop(&mut self) {
        // process_group(0) で作った、この呼出し所有のグループだけを止める。
        // SAFETY: 負の PID はプロセスグループへの送信。終了済みでも害はない。
        unsafe {
            libc::kill(-(self.pid as i32), libc::SIGKILL);
        }
    }
}

/// `command` を上限 `timeout` で実行し、成功（終了コード 0）なら標準出力を返す。
/// 標準入力は閉じ（CLI が対話の入力を待って固まらないため）、標準出力・標準エラーはそれぞれ `limit`
/// バイトまで読む。上限時間・出力上限を超えたら子孫ごと止めて失敗にする。
pub fn run(mut command: Command, timeout: Duration, limit: u64) -> Result<Vec<u8>, RunError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    Tree::prepare(&mut command);
    let mut child = command.spawn().map_err(|_| RunError::Spawn)?;
    let _tree = Tree::attach(&mut child)?;
    fn read_pipe(
        pipe: impl Read + Send + 'static,
        limit: u64,
    ) -> std::sync::mpsc::Receiver<std::io::Result<Vec<u8>>> {
        let (sender, reader) = std::sync::mpsc::sync_channel(1);
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = pipe.take(limit + 1).read_to_end(&mut bytes).map(|_| bytes);
            let _ = sender.send(result);
        });
        reader
    }
    let output = read_pipe(child.stdout.take().ok_or(RunError::Pipe)?, limit);
    let errors = read_pipe(child.stderr.take().ok_or(RunError::Pipe)?, limit);
    let deadline = Instant::now() + timeout;
    let mut stdout = None;
    let mut stderr = None;
    loop {
        for (channel, target) in [(&output, &mut stdout), (&errors, &mut stderr)] {
            if target.is_none() {
                match channel.try_recv() {
                    Ok(Ok(bytes)) => {
                        if bytes.len() as u64 > limit {
                            let _ = child.kill();
                            let _ = child.wait();
                            return Err(RunError::OutputTooLarge);
                        }
                        *target = Some(bytes);
                    }
                    Ok(Err(_)) => return Err(RunError::Pipe),
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                    Err(_) => return Err(RunError::Pipe),
                }
            }
        }
        if let Some(status) = child.try_wait().map_err(|_| RunError::Pipe)? {
            if !status.success() {
                return Err(RunError::Failed);
            }
            if let (Some(_), Some(_)) = (&stdout, &stderr) {
                return Ok(stdout.unwrap_or_default());
            }
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(RunError::Timeout);
        }
        thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Windows の標準コマンド（cmd.exe）で、成功・失敗・時間切れ・出力上限を確かめる。
    #[cfg(windows)]
    fn cmd(script: &str) -> Command {
        let system = std::env::var_os("SystemRoot").unwrap();
        // cmd.exe は自分のコマンドラインを独自に解析し、パス中の `/` をスイッチと誤解するため `\` で連結する。
        let mut command = Command::new(
            std::path::Path::new(&system)
                .join("System32")
                .join("cmd.exe"),
        );
        command.args(["/d", "/c", script]);
        command
    }

    #[cfg(windows)]
    #[test]
    fn success_failure_timeout_and_output_bounds() {
        assert_eq!(
            String::from_utf8(run(cmd("echo ok"), Duration::from_secs(10), 1024).unwrap())
                .unwrap()
                .trim(),
            "ok"
        );
        assert_eq!(
            run(cmd("exit 3"), Duration::from_secs(10), 1024),
            Err(RunError::Failed)
        );
        // ping で約5秒待つ子を、1秒で子孫ごと止める。
        let started = Instant::now();
        assert_eq!(
            run(
                cmd("ping -n 6 127.0.0.1 >nul"),
                Duration::from_secs(1),
                1024
            ),
            Err(RunError::Timeout)
        );
        assert!(started.elapsed() < Duration::from_secs(4));
        assert_eq!(
            run(
                cmd("for /l %i in (1,1,400) do @echo 0123456789"),
                Duration::from_secs(10),
                64
            ),
            Err(RunError::OutputTooLarge)
        );
    }
}
