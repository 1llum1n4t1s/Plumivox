#[path = "windows_junction.rs"]
mod windows_junction;
use super::*;
use std::{
    io::{Read, Write},
    os::windows::io::{AsRawHandle, BorrowedHandle},
    sync::mpsc,
    time::Instant,
};
use windows_junction::directory_junction;

fn acl(path: &Path, broad: bool) -> Result<()> {
    use std::{
        os::windows::ffi::OsStrExt,
        ptr::{null, null_mut},
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, LocalFree},
        Security::{Authorization::*, *},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    ensure!(
        !fs::symlink_metadata(path)?.file_type().is_symlink(),
        "ACL fixture must not follow links"
    );
    let directory = fs::metadata(path)?.is_dir();
    let path: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    unsafe {
        let mut token = null_mut();
        ensure!(OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) != 0);
        let mut required = 0;
        GetTokenInformation(token, TokenUser, null_mut(), 0, &mut required);
        let mut user = vec![0u8; required as usize];
        let result = GetTokenInformation(
            token,
            TokenUser,
            user.as_mut_ptr().cast(),
            required,
            &mut required,
        );
        CloseHandle(token);
        ensure!(result != 0);
        let sid = (*(user.as_ptr().cast::<TOKEN_USER>())).User.Sid;
        let mut owner = null_mut();
        let mut original = null_mut();
        ensure!(
            GetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                null_mut(),
                null_mut(),
                &mut original
            ) == 0
        );
        let owned = EqualSid(owner, sid) != 0;
        LocalFree(original);
        ensure!(owned, "ACL fixture must belong to current user");
        let mut sid_text = null_mut();
        ensure!(ConvertSidToStringSidW(sid, &mut sid_text) != 0);
        let mut len = 0;
        while *sid_text.add(len) != 0 {
            len += 1;
        }
        let sid_string = String::from_utf16(std::slice::from_raw_parts(sid_text, len))?;
        LocalFree(sid_text.cast());
        let sddl = format!(
            "D:P(A;OICI;FA;;;{sid_string}){}",
            if broad {
                if directory {
                    "(A;OICI;FR;;;WD)"
                } else {
                    "(A;;FR;;;WD)"
                }
            } else {
                ""
            }
        );
        let sddl: Vec<u16> = sddl.encode_utf16().chain([0]).collect();
        let mut descriptor = null_mut();
        ensure!(
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                null_mut()
            ) != 0
        );
        let mut present = 0;
        let mut defaulted = 0;
        let mut dacl = null_mut();
        let got = GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted);
        let status = if got != 0 && present != 0 {
            SetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                dacl,
                null(),
            )
        } else {
            1
        };
        LocalFree(descriptor);
        ensure!(status == 0, "Cannot set fixture ACL");
    }
    Ok(())
}
fn write_private(path: &Path, value: &Value) -> Result<()> {
    fs::write(path, serde_json::to_vec(value)?)?;
    acl(path, false)
}
fn assert_inherited_public_read(path: &Path) -> Result<()> {
    use std::{os::windows::ffi::OsStrExt, ptr::null_mut};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{Authorization::*, *},
    };
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    unsafe {
        let mut descriptor = null_mut();
        let mut dacl = null_mut();
        ensure!(
            GetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                &mut dacl,
                null_mut(),
                &mut descriptor
            ) == 0
        );
        let mut everyone = null_mut();
        let text: Vec<u16> = "S-1-1-0".encode_utf16().chain([0]).collect();
        if ConvertStringSidToSidW(text.as_ptr(), &mut everyone) == 0 {
            LocalFree(descriptor);
            anyhow::bail!("Cannot construct Everyone SID");
        }
        let found = if dacl.is_null() {
            false
        } else {
            (0..(*dacl).AceCount).any(|index| {
                let mut raw = null_mut();
                if GetAce(dacl, index as u32, &mut raw) == 0 {
                    return false;
                }
                let ace = &*(raw.cast::<ACCESS_ALLOWED_ACE>());
                ace.Header.AceType == 0
                    && ace.Header.AceFlags & 0x10 != 0
                    && ace.Mask & 1 != 0
                    && EqualSid((&ace.SidStart as *const u32).cast_mut().cast(), everyone) != 0
            })
        };
        LocalFree(everyone);
        LocalFree(descriptor);
        ensure!(
            found,
            "Fixture child must actually inherit Everyone read access"
        );
    }
    Ok(())
}
fn assert_private_acl(path: &Path) -> Result<()> {
    use std::{os::windows::ffi::OsStrExt, ptr::null_mut};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, LocalFree},
        Security::{Authorization::*, *},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    unsafe {
        let mut token = null_mut();
        ensure!(OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) != 0);
        let mut required = 0;
        GetTokenInformation(token, TokenUser, null_mut(), 0, &mut required);
        let mut user = vec![0u8; required as usize];
        let result = GetTokenInformation(
            token,
            TokenUser,
            user.as_mut_ptr().cast(),
            required,
            &mut required,
        );
        CloseHandle(token);
        ensure!(result != 0);
        let sid = (*(user.as_ptr().cast::<TOKEN_USER>())).User.Sid;
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
            ) == 0
        );
        // 設定関数は呼ばず、製品が作った実ACLを独立に読む。
        let valid = (|| -> bool {
            if EqualSid(owner, sid) == 0 || dacl.is_null() || (*dacl).AceCount != 1 {
                return false;
            }
            let mut control = 0;
            let mut revision = 0;
            if GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) == 0
                || control & SE_DACL_PROTECTED == 0
            {
                return false;
            }
            let mut ace = null_mut();
            if GetAce(dacl, 0, &mut ace) == 0 {
                return false;
            }
            let allowed = &*(ace.cast::<ACCESS_ALLOWED_ACE>());
            allowed.Header.AceType == 0
                && allowed.Header.AceFlags & 0x10 == 0
                && allowed.Mask & 0x1f01ff == 0x1f01ff
                && EqualSid((&allowed.SidStart as *const u32).cast_mut().cast(), sid) != 0
        })();
        LocalFree(descriptor);
        ensure!(
            valid,
            "Generated path is not protected with a single explicit current-user full-access ACE: {}",
            path.display()
        );
    }
    Ok(())
}

// 専用fixtureだけがコンソールを割り当てる。本番の端末入力を差し替えない。
#[repr(C)]
struct KeyEvent {
    down: i32,
    repeat: u16,
    virtual_key: u16,
    scan: u16,
    unicode: u16,
    control: u32,
}
#[repr(C)]
struct InputRecord {
    kind: u16,
    padding: u16,
    event: KeyEvent,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Coord {
    x: i16,
    y: i16,
}
#[repr(C)]
#[derive(Default)]
struct ConsoleInfo {
    size: Coord,
    cursor: Coord,
    attributes: u16,
    window: [i16; 4],
    maximum: Coord,
}
#[repr(C)]
#[derive(Default)]
struct JobBasic {
    process_time: i64,
    job_time: i64,
    flags: u32,
    minimum: usize,
    maximum: usize,
    active: u32,
    affinity: usize,
    priority: u32,
    scheduling: u32,
}
#[repr(C)]
#[derive(Default)]
struct JobLimits {
    basic: JobBasic,
    io: [u64; 6],
    process_memory: usize,
    job_memory: usize,
    peak_process: usize,
    peak_job: usize,
}
struct FixtureJob(*mut std::ffi::c_void);
impl Drop for FixtureJob {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
struct ConsoleCodePage(u32);
impl Drop for ConsoleCodePage {
    fn drop(&mut self) {
        unsafe {
            SetConsoleOutputCP(self.0);
        }
    }
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn FreeConsole() -> i32;
    fn AllocConsole() -> i32;
    fn GetStdHandle(kind: u32) -> *mut std::ffi::c_void;
    fn GetConsoleOutputCP() -> u32;
    fn SetConsoleOutputCP(code_page: u32) -> i32;
    fn MultiByteToWideChar(
        code_page: u32,
        flags: u32,
        input: *const u8,
        input_length: i32,
        output: *mut u16,
        output_length: i32,
    ) -> i32;
    fn GetShortPathNameW(long: *const u16, short: *mut u16, length: u32) -> u32;
    fn GetConsoleMode(handle: *mut std::ffi::c_void, mode: *mut u32) -> i32;
    fn GetConsoleScreenBufferInfo(handle: *mut std::ffi::c_void, info: *mut ConsoleInfo) -> i32;
    fn ReadConsoleOutputCharacterW(
        handle: *mut std::ffi::c_void,
        buffer: *mut u16,
        length: u32,
        origin: Coord,
        read: *mut u32,
    ) -> i32;
    fn CreateJobObjectW(
        attributes: *const std::ffi::c_void,
        name: *const u16,
    ) -> *mut std::ffi::c_void;
    fn SetInformationJobObject(
        job: *mut std::ffi::c_void,
        class: i32,
        info: *const std::ffi::c_void,
        length: u32,
    ) -> i32;
    fn AssignProcessToJobObject(job: *mut std::ffi::c_void, process: *mut std::ffi::c_void) -> i32;
    fn WriteConsoleInputW(
        handle: *mut std::ffi::c_void,
        records: *const InputRecord,
        length: u32,
        written: *mut u32,
    ) -> i32;
    fn SetConsoleCtrlHandler(
        handler: Option<unsafe extern "system" fn(u32) -> i32>,
        add: i32,
    ) -> i32;
    fn GenerateConsoleCtrlEvent(event: u32, group: u32) -> i32;
    fn GetNumberOfConsoleInputEvents(handle: *mut std::ffi::c_void, count: *mut u32) -> i32;
}
unsafe extern "system" fn fixture_ctrl_handler(event: u32) -> i32 {
    i32::from(event == 0)
}
struct FixtureCtrlHandler;
impl Drop for FixtureCtrlHandler {
    fn drop(&mut self) {
        unsafe {
            SetConsoleCtrlHandler(Some(fixture_ctrl_handler), 0);
        }
    }
}

fn fixture_job(process: *mut std::ffi::c_void) -> Result<FixtureJob> {
    let job = FixtureJob(unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) });
    ensure!(!job.0.is_null(), "Cannot create fixture process job");
    let mut limits = JobLimits::default();
    limits.basic.flags = 0x2000; // JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
    ensure!(
        unsafe {
            SetInformationJobObject(
                job.0,
                9,
                (&limits as *const JobLimits).cast(),
                std::mem::size_of::<JobLimits>() as u32,
            )
        } != 0,
        "Cannot configure fixture process cleanup"
    );
    ensure!(
        unsafe { AssignProcessToJobObject(job.0, process) } != 0,
        "Cannot assign fixture process job"
    );
    Ok(job)
}

struct SetupChild(std::process::Child);
impl Drop for SetupChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn wait_until(
    child: &mut std::process::Child,
    deadline: Instant,
) -> Result<std::process::ExitStatus> {
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        ensure!(
            Instant::now() < deadline,
            "Setup process did not exit before deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn bounded_output(command: &mut std::process::Command) -> Result<std::process::Output> {
    let mut child = SetupChild(
        command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?,
    );
    let status = wait_until(&mut child.0, Instant::now() + Duration::from_secs(5))?;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    child.0.stdout.take().unwrap().read_to_end(&mut stdout)?;
    child.0.stderr.take().unwrap().read_to_end(&mut stderr)?;
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

fn console_input() -> Result<fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    Ok(fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(3)
        .open("CONIN$")?)
}
fn utf8_interpreted_as_cp932(value: &str) -> Option<String> {
    let length = unsafe {
        MultiByteToWideChar(
            932,
            0,
            value.as_ptr(),
            value.len() as i32,
            std::ptr::null_mut(),
            0,
        )
    };
    if length <= 0 {
        return None;
    }
    let mut output = vec![0u16; length as usize];
    if unsafe {
        MultiByteToWideChar(
            932,
            0,
            value.as_ptr(),
            value.len() as i32,
            output.as_mut_ptr(),
            length,
        )
    } != length
    {
        return None;
    }
    String::from_utf16(&output).ok()
}
fn console_text(console: &fs::File) -> Result<String> {
    let mut info = ConsoleInfo::default();
    ensure!(
        unsafe { GetConsoleScreenBufferInfo(console.as_raw_handle(), &mut info) } != 0,
        "Cannot inspect fixture console output"
    );
    let length = (i32::from(info.cursor.y) * i32::from(info.size.x) + i32::from(info.cursor.x))
        .max(0) as u32;
    ensure!(
        length <= 1024 * 1024,
        "Fixture console output exceeded limit"
    );
    if length == 0 {
        return Ok(String::new());
    }
    let mut buffer = vec![0u16; length as usize];
    let mut read = 0;
    ensure!(
        unsafe {
            ReadConsoleOutputCharacterW(
                console.as_raw_handle(),
                buffer.as_mut_ptr(),
                length,
                Coord::default(),
                &mut read,
            )
        } != 0,
        "Cannot read fixture console output"
    );
    Ok(String::from_utf16_lossy(&buffer[..read as usize]))
}
fn send_console(console: &fs::File, value: &str, secret: bool, submit: bool) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut mode = 0;
        ensure!(unsafe { GetConsoleMode(console.as_raw_handle(), &mut mode) } != 0);
        if (mode & 4 == 0) == secret {
            break;
        }
        ensure!(
            Instant::now() < deadline,
            "Console echo mode did not match input secrecy"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let records: Vec<_> = value
        .encode_utf16()
        .chain(submit.then_some(13))
        .map(|unicode| InputRecord {
            kind: 1,
            padding: 0,
            event: KeyEvent {
                down: 1,
                repeat: 1,
                virtual_key: if unicode == 13 { 13 } else { 0 },
                scan: 0,
                unicode,
                control: 0,
            },
        })
        .collect();
    let mut written = 0;
    ensure!(
        unsafe {
            WriteConsoleInputW(
                console.as_raw_handle(),
                records.as_ptr(),
                records.len() as u32,
                &mut written,
            )
        } != 0
            && written as usize == records.len()
    );
    Ok(())
}

#[test]
#[ignore = "Dedicated subprocess fixture, invoked explicitly by the setup suite"]
fn terminal_fixture() -> Result<()> {
    if std::env::var_os("PLUMIVOX_WINDOWS_TERMINAL_FIXTURE").is_none() {
        return Ok(());
    }
    let request: Value = serde_json::from_reader(std::io::stdin())?;
    // AllocConsoleで標準ハンドルが変わっても、結果は親の元のpipeへ返す。
    let output_handle = unsafe { GetStdHandle((-11i32) as u32) };
    ensure!(
        !output_handle.is_null() && output_handle as isize != -1,
        "Missing fixture output pipe"
    );
    let mut result_output =
        fs::File::from(unsafe { BorrowedHandle::borrow_raw(output_handle) }.try_clone_to_owned()?);
    unsafe {
        FreeConsole();
    }
    ensure!(
        unsafe { AllocConsole() } != 0,
        "Cannot allocate dedicated fixture console"
    );
    // fixture自身だけは制御イベントを受け流す。子にはこのcallbackは継承されない。
    ensure!(
        unsafe { SetConsoleCtrlHandler(Some(fixture_ctrl_handler), 1) } != 0,
        "Cannot protect dedicated fixture from its synthetic control event"
    );
    let _ctrl_handler = FixtureCtrlHandler;
    let original_code_page = unsafe { GetConsoleOutputCP() };
    ensure!(
        original_code_page != 0,
        "Cannot inspect fixture console output code page"
    );
    let code_page_guard = ConsoleCodePage(original_code_page);
    if let Some(code_page) = request["console_output_cp"].as_u64() {
        ensure!(
            unsafe { SetConsoleOutputCP(code_page as u32) } != 0,
            "Cannot configure fixture console output code page"
        );
        ensure!(
            unsafe { GetConsoleOutputCP() } == code_page as u32,
            "Fixture console did not use requested code page"
        );
    }
    let setup_code_page = unsafe { GetConsoleOutputCP() };
    let console = console_input()?;
    let mut initial_mode = 0;
    ensure!(
        unsafe { GetConsoleMode(console.as_raw_handle(), &mut initial_mode) } != 0
            && initial_mode & 4 != 0,
        "Fixture console must initially echo normal input"
    );
    let console_output = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("CONOUT$")?;
    let mut command = std::process::Command::new(request["binary"].as_str().unwrap());
    command.args(
        request["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap()),
    );
    for name in [
        "PLUMIVOX_SECRETS_FILE",
        "PLUMIVOX_DEFAULT_ACCOUNT",
        "PLUMIVOX_STATE_DIR",
        "PLUMIVOX_TEST_HOOKS",
        "PLUMIVOX_WINDOWS_TERMINAL_FIXTURE",
    ] {
        command.env_remove(name);
    }
    for (name, value) in request["env"].as_object().unwrap() {
        command.env(name, value.as_str().unwrap());
    }
    command
        .stdin(Stdio::from(console.try_clone()?))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = SetupChild(command.spawn()?);
    let (tx, rx) = mpsc::channel();
    for mut source in [
        Box::new(child.0.stdout.take().unwrap()) as Box<dyn Read + Send>,
        Box::new(child.0.stderr.take().unwrap()),
    ] {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut bytes = [0; 4096];
            while let Ok(n) = source.read(&mut bytes) {
                if n == 0 {
                    break;
                }
                if tx.send(bytes[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
    }
    drop(tx);
    let answers = request["answers"].as_array().unwrap();
    let mut answered = 0;
    let mut transcript = Vec::new();
    let mut pending = Vec::new();
    let mut previous_screen = String::new();
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        if Instant::now() >= deadline {
            let mut mode = 0;
            let mode_ok = unsafe { GetConsoleMode(console.as_raw_handle(), &mut mode) } != 0;
            let expected = answers
                .get(answered)
                .and_then(|row| row[0].as_str())
                .unwrap_or("<process exit>");
            let correct_prompt = previous_screen.contains(expected);
            let cp932_mojibake = utf8_interpreted_as_cp932(expected)
                .is_some_and(|value| previous_screen.contains(&value));
            anyhow::bail!(
                "Native terminal setup timed out: answered={answered}/{}, expected_prompt={expected:?}, child_status={:?}, input_mode={mode_ok}:{mode}, output_code_page={}, pipe_and_console_bytes={}, screen_bytes={}, expected_prompt_in_screen={correct_prompt}, utf8_as_cp932_prompt_in_screen={cp932_mojibake}",
                answers.len(),
                child.0.try_wait()?,
                unsafe { GetConsoleOutputCP() },
                transcript.len(),
                previous_screen.len()
            );
        }
        let disconnected = match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(bytes) => {
                transcript.extend_from_slice(&bytes);
                pending.extend_from_slice(&bytes);
                false
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => true,
            Err(mpsc::RecvTimeoutError::Timeout) => false,
        };
        // Windowsの秘密入力（src/setup_windows_console.rsのWriteConsoleW）はCONOUT$へ直接promptを書くため、pipeと専用画面の両方を観測する。
        let screen = console_text(&console_output)?;
        let fresh = screen.strip_prefix(&previous_screen).unwrap_or(&screen);
        transcript.extend_from_slice(fresh.as_bytes());
        pending.extend_from_slice(fresh.as_bytes());
        previous_screen = screen;
        ensure!(
            transcript.len() < 1024 * 1024,
            "Terminal output exceeded limit"
        );
        if disconnected {
            break;
        }
        while let Some(row) = answers.get(answered) {
            let prompt = row[0].as_str().unwrap().as_bytes();
            let Some(at) = pending.windows(prompt.len()).position(|s| s == prompt) else {
                break;
            };
            pending.drain(..at + prompt.len());
            if request["mutate_at"].as_u64() == Some(answered as u64) {
                fs::write(
                    request["mutate_path"].as_str().unwrap(),
                    request["mutate_value"].as_str().unwrap(),
                )?;
            }
            if request["busy_at"].as_u64() == Some(answered as u64) {
                let busy_args = request.get("busy_args").unwrap_or(&request["args"]);
                let args: Vec<_> = busy_args
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap())
                    .collect();
                let out = bounded_output(
                    std::process::Command::new(request["binary"].as_str().unwrap())
                        .args(args)
                        .stdin(Stdio::null()),
                )?;
                ensure!(
                    !out.status.success() && text(&out).contains("SETUP_BUSY"),
                    "Concurrent setup was not rejected"
                );
            }
            let value = row[1].as_str().unwrap();
            let cancel = request["cancel_at"].as_u64() == Some(answered as u64);
            send_console(&console, value, row[2] == true, !cancel)?;
            if cancel {
                ensure!(
                    row[2] == true,
                    "Cancellation fixture must stop at a secret input"
                );
                let deadline = Instant::now() + Duration::from_secs(5);
                loop {
                    let mut queued = 0;
                    ensure!(
                        unsafe {
                            GetNumberOfConsoleInputEvents(console.as_raw_handle(), &mut queued)
                        } != 0
                    );
                    if queued == 0 {
                        break;
                    }
                    ensure!(
                        Instant::now() < deadline,
                        "Secret input was not consumed before cancellation"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
                ensure!(
                    unsafe { GenerateConsoleCtrlEvent(0, 0) } != 0,
                    "Cannot deliver synthetic Ctrl+C"
                );
            }
            answered += 1;
        }
    }
    let status = wait_until(&mut child.0, deadline)?;
    let mut final_mode = 0;
    ensure!(
        unsafe { GetConsoleMode(console.as_raw_handle(), &mut final_mode) } != 0
            && final_mode == initial_mode,
        "Setup did not restore console input mode"
    );
    ensure!(
        unsafe { GetConsoleOutputCP() } == setup_code_page,
        "Setup changed the console output code page"
    );
    let output = String::from_utf8_lossy(&transcript);
    for value in request["secrets"].as_array().unwrap() {
        ensure!(
            !output.contains(value.as_str().unwrap()),
            "Terminal echoed secret input"
        );
    }
    drop(code_page_guard);
    ensure!(
        unsafe { GetConsoleOutputCP() } == original_code_page,
        "Fixture did not restore the original console output code page"
    );
    writeln!(
        result_output,
        "PLUMIVOX_RESULT={}",
        json!({"output":output,"answered":answered,"code":status.code(),"setupCodePage":setup_code_page,"restoredCodePage":original_code_page,"inputModeRestored":true})
    )?;
    Ok(())
}

async fn terminal(mut request: Value) -> Result<Value> {
    request["binary"] = json!(binary());
    let mut command = tokio::process::Command::new(std::env::current_exe()?);
    clean(&mut command);
    command
        .args([
            "--exact",
            "windows::terminal_fixture",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("PLUMIVOX_WINDOWS_TERMINAL_FIXTURE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    // request受信までfixtureは製品を起動しない。先にJobへ所属させ、timeout時も全子孫を回収する。
    let _job = fixture_job(
        child
            .raw_handle()
            .context("Missing fixture process handle")?,
    )?;
    use tokio::io::AsyncWriteExt;
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(&serde_json::to_vec(&request)?).await?;
    drop(stdin);
    let out = tokio::time::timeout(Duration::from_secs(35), child.wait_with_output()).await??;
    ensure!(
        out.status.success(),
        "Console fixture failed: {}",
        text(&out)
    );
    let output = text(&out);
    let row = output
        .lines()
        .find_map(|line| line.find("PLUMIVOX_RESULT=").map(|i| &line[i + 16..]))
        .context("Missing console fixture result")?;
    Ok(serde_json::from_str(row)?)
}

fn answers(secrets: &[String]) -> Value {
    json!([
        ["Xユーザー名 (@なし): ", "example_user", false],
        ["VPSホスト名/IP: ", "example.invalid", false],
        ["SSHユーザー [plumivox]: ", "", false],
        ["SSHポート [22]: ", "", false],
        ["PC内部ポート [43128]: ", "", false],
        ["VPS内部ポート [43129]: ", "", false],
        [
            "コンソールで確認済みのSHA256指紋: ",
            format!("SHA256:{}", "A".repeat(43)),
            false
        ],
        ["公開プロキシポート [3128]: ", "", false],
        ["プロキシユーザー [plumivox]: ", "", false],
        ["twitterapi.io APIキー: ", secrets[0], true],
        ["Xログインメール: ", secrets[1], true],
        ["Xパスワード: ", secrets[2], true],
        ["Base32の2FA秘密鍵 (未使用なら空欄): ", secrets[4], true],
        ["プロキシパスワード: ", secrets[3], true]
    ])
}
fn assert_registered_settings(doc: &Value, key: &Path, secrets: &[String]) -> Result<()> {
    let profile = &doc["twitterapi_io"]["main"];
    ensure!(
        profile["user_name"] == "example_user"
            && profile["api_key"] == secrets[0]
            && profile["email"] == secrets[1]
            && profile["password"] == secrets[2]
            && profile["totp_secret"] == secrets[4]
            && profile["proxy_mode"] == "managed",
        "Saved main account does not match wizard input"
    );
    let proxy = url::Url::parse(profile["proxy"].as_str().context("Missing saved proxy")?)?;
    ensure!(
        proxy.scheme() == "http"
            && proxy.username() == "plumivox"
            && proxy.password() == Some(secrets[3].as_str())
            && proxy.host_str() == Some("example.invalid")
            && proxy.port() == Some(3128),
        "Saved proxy does not match wizard input"
    );
    ensure!(
        doc["plumivox"]["managed_proxy"]
            == json!({"ssh_host":"example.invalid","ssh_user":"plumivox","ssh_port":22,"local_port":43128,"remote_port":43129,"private_key_file":key,"host_key_sha256":format!("SHA256:{}","A".repeat(43))}),
        "Saved managed relay does not match wizard input"
    );
    Ok(())
}

async fn cli(
    args: &[String],
    env: &[(String, String)],
    expected: i32,
    secrets: &[String],
) -> Result<String> {
    let mut all = vec!["--setup".into()];
    all.extend_from_slice(args);
    let out = execute(&binary(), &all, env).await?;
    let output = text(&out);
    no_leak(&output, secrets)?;
    ensure!(
        out.status.code() == Some(expected),
        "Native setup unexpected result: {output}"
    );
    Ok(output)
}

fn short_alias(path: &Path) -> Result<Option<PathBuf>> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let required = unsafe { GetShortPathNameW(wide.as_ptr(), std::ptr::null_mut(), 0) };
    // 長い名前のまま返るNTFS設定でも、API失敗でも短縮名の検証済みとは扱わない。
    if required == 0 {
        return Ok(None);
    }
    let mut buffer = vec![0u16; required as usize];
    let length = unsafe { GetShortPathNameW(wide.as_ptr(), buffer.as_mut_ptr(), required) };
    ensure!(
        length > 0 && length < required,
        "Short path changed while reading fixture alias"
    );
    let alias = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length as usize]));
    if alias
        .file_name()
        .unwrap()
        .to_string_lossy()
        .eq_ignore_ascii_case(&path.file_name().unwrap().to_string_lossy())
    {
        return Ok(None);
    }
    ensure!(
        fs::canonicalize(&alias)? == fs::canonicalize(path)?,
        "Short alias must identify fixture directory"
    );
    Ok(Some(alias))
}

pub async fn run(suite: &mut Suite) -> Result<()> {
    let secrets = [
        "dummy-api-secret",
        "dummy-email-secret",
        "dummy-x-password",
        "dummy-proxy-password",
        "JBSWY3DPEHPK3PXP",
    ]
    .map(String::from)
    .to_vec();
    for secret in &secrets {
        suite.add_secret(secret.clone());
    }
    // 広い本人所有parentから作成し、親の狭いACLの継承だけで合格させない。
    let wide_parent = suite.output().join("broad-owned-parent");
    fs::create_dir(&wide_parent)?;
    acl(&wide_parent, true)?;
    let key = wide_parent.join("keys/plumivox_ed25519");
    let settings = wide_parent.join("native-settings.json");
    macro_rules! case {($name:expr,$body:block)=>{{let result:Result<()>=async $body.await;suite.record($name,result);}};}
    case!("native-broad-parent-actually-inherits-everyone-read", {
        let probe = wide_parent.join("inherited-probe.txt");
        fs::write(&probe, "non-secret fixture")?;
        assert_inherited_public_read(&probe)?;
        Ok(())
    });
    case!("native-prepare-key-and-refuse-replacement", {
        let args = vec![
            "--prepare-key".into(),
            "--private-key-path".into(),
            key.display().to_string(),
        ];
        ensure!(cli(&args, &[], 0, &secrets).await?.contains("ssh-ed25519 "));
        for path in [
            key.parent().unwrap().to_path_buf(),
            key.clone(),
            key.with_extension("pub"),
        ] {
            assert_private_acl(&path)?;
        }
        let before = fs::read(&key)?;
        let public = fs::read(key.with_extension("pub"))?;
        ensure!(cli(&args, &[], 1, &secrets).await?.contains("KEY_EXISTS"));
        ensure!(fs::read(&key)? == before && fs::read(key.with_extension("pub"))? == public);
        Ok(())
    });
    let args = |target: &Path| {
        vec![
            "--setup".to_string(),
            "--settings-path".into(),
            target.display().to_string(),
            "--private-key-path".into(),
            key.display().to_string(),
        ]
    };
    let wizard = |target: &Path| json!({"args":args(target),"env":{},"answers":answers(&secrets),"secrets":secrets});
    case!(
        "native-wizard-hidden-secrets-first-default-and-check-readonly",
        {
            let out = terminal(wizard(&settings)).await?;
            ensure!(
                out["code"] == 0 && out["answered"] == 14,
                "Wizard failed: {}",
                out["output"]
            );
            let doc = document(&settings)?;
            assert_private_acl(&settings)?;
            ensure!(
                doc["twitterapi_io"]["default_account"] == "main"
                    && doc["twitterapi_io"]["main"]["api_key"] == secrets[0]
            );
            ensure!(
                doc["twitterapi_io"]["main"]["email"] == secrets[1]
                    && doc["twitterapi_io"]["main"]["password"] == secrets[2]
                    && doc["twitterapi_io"]["main"]["totp_secret"] == secrets[4]
            );
            ensure!(
                doc["twitterapi_io"]["main"]["user_name"] == "example_user"
                    && doc["twitterapi_io"]["main"]["proxy_mode"] == "managed"
            );
            let relay = &doc["plumivox"]["managed_proxy"];
            ensure!(
                relay
                    == &json!({"ssh_host":"example.invalid","ssh_user":"plumivox","ssh_port":22,"local_port":43128,"remote_port":43129,"private_key_file":key,"host_key_sha256":format!("SHA256:{}","A".repeat(43))})
            );
            ensure!(
                url::Url::parse(doc["twitterapi_io"]["main"]["proxy"].as_str().unwrap())?
                    .password()
                    == Some(secrets[3].as_str())
            );
            let proxy = url::Url::parse(doc["twitterapi_io"]["main"]["proxy"].as_str().unwrap())?;
            ensure!(
                proxy.scheme() == "http"
                    && proxy.username() == "plumivox"
                    && proxy.host_str() == Some("example.invalid")
                    && proxy.port() == Some(3128)
            );
            let before = fs::read(&settings)?;
            cli(
                &[
                    "--check".into(),
                    "--settings-path".into(),
                    settings.display().to_string(),
                ],
                &[],
                0,
                &secrets,
            )
            .await?;
            ensure!(fs::read(&settings)? == before);
            ensure!(terminal(wizard(&settings)).await?["code"] == 1);
            ensure!(fs::read(&settings)? == before);
            Ok(())
        }
    );
    case!(
        "native-wizard-cp932-japanese-secret-prompts-and-console-restoration",
        {
            let target = wide_parent.join("cp932-settings.json");
            let mut request = wizard(&target);
            request["console_output_cp"] = json!(932);
            let out = terminal(request).await?;
            ensure!(
                out["code"] == 0
                    && out["answered"] == 14
                    && out["setupCodePage"] == 932
                    && out["inputModeRestored"] == true,
                "CP932 native wizard failed: {}",
                out["output"]
            );
            let transcript = out["output"].as_str().context("Missing CP932 transcript")?;
            // 生のCONOUT画面から全日本語promptが一致した場合だけ14件の入力が完了する。
            for row in answers(&secrets).as_array().unwrap() {
                ensure!(
                    transcript.contains(row[0].as_str().unwrap()),
                    "CP932 console did not preserve a Japanese setup prompt"
                );
            }
            no_leak(transcript, &secrets)?;
            let doc = document(&target)?;
            let profile = &doc["twitterapi_io"]["main"];
            ensure!(
                profile["api_key"] == secrets[0]
                    && profile["email"] == secrets[1]
                    && profile["password"] == secrets[2]
                    && profile["totp_secret"] == secrets[4]
            );
            ensure!(
                url::Url::parse(profile["proxy"].as_str().unwrap())?.password()
                    == Some(secrets[3].as_str())
            );
            assert_private_acl(&target)?;
            Ok(())
        }
    );
    case!(
        "native-secret-ctrl-c-restores-console-without-echo-or-save",
        {
            let target = wide_parent.join("ctrl-c-must-not-be-saved.json");
            let mut request = wizard(&target);
            request["answers"].as_array_mut().unwrap().truncate(10);
            request["cancel_at"] = json!(9);
            let out = terminal(request).await?;
            ensure!(
                out["code"] != 0 && out["answered"] == 10 && out["inputModeRestored"] == true,
                "Ctrl+C did not stop the wizard and restore its console"
            );
            no_leak(
                out["output"]
                    .as_str()
                    .context("Missing Ctrl+C transcript")?,
                &secrets,
            )?;
            ensure!(!target.exists(), "Cancelled wizard saved settings");
            Ok(())
        }
    );
    case!("native-wizard-observes-concurrent-change-and-lock", {
        for existing in [false, true] {
            let target = suite
                .output()
                .join(format!("race-existing-{existing}.json"));
            if existing {
                write_private(&target, &json!({"unrelated":"original"}))?;
            }
            let changed = "{\"unrelated\":\"concurrent-change\"}";
            let mut request = wizard(&target);
            request["mutate_at"] = json!(13);
            request["mutate_path"] = json!(target);
            request["mutate_value"] = json!(changed);
            request["busy_at"] = json!(0);
            let out = terminal(request).await?;
            ensure!(
                out["code"] == 1 && out["output"].as_str().unwrap().contains("SETTINGS_CHANGED")
            );
            ensure!(fs::read_to_string(&target)? == changed);
        }
        Ok(())
    });
    let alias_lock_root = suite.output().join("setup-lock-directory-with-long-name");
    fs::create_dir(&alias_lock_root)?;
    if let Some(short_root) = short_alias(&alias_lock_root)? {
        let mut completed = 0;
        for short_first in [false, true] {
            let name = if short_first {
                "native-setup-short-name-lock-blocks-long-name"
            } else {
                "native-setup-long-name-lock-blocks-short-name"
            };
            case!(name, {
                let filename = format!("alias-lock-{short_first}.json");
                let long_target = alias_lock_root.join(&filename);
                let short_target = short_root.join(&filename);
                let (primary, competing) = if short_first {
                    (&short_target, &long_target)
                } else {
                    (&long_target, &short_target)
                };
                let mut request = wizard(primary);
                request["busy_at"] = json!(0);
                request["busy_args"] = json!(args(competing));
                let out = terminal(request).await?;
                ensure!(
                    out["code"] == 0 && out["answered"] == 14,
                    "Alias lock wizard failed: {}",
                    out["output"]
                );
                // 所有プロセス終了後には競合側の表記でも同じ設定を開ける。
                cli(
                    &[
                        "--check".into(),
                        "--settings-path".into(),
                        competing.display().to_string(),
                    ],
                    &[],
                    0,
                    &secrets,
                )
                .await?;
                ensure!(document(&long_target)? == document(&short_target)?);
                completed += 1;
                Ok(())
            });
        }
        suite.write_json("setup-lock-alias-coverage.json",&json!({"shortAliasLock":if completed==2 {"verified"} else {"failed"},"completedDirections":completed,"directions":["long-to-short","short-to-long"]}))?;
    } else {
        suite.write_json("setup-lock-alias-coverage.json",&json!({"shortAliasLock":"notRun","reason":"Fixture directory has no usable distinct 8.3 alias; alias lock cases are excluded from suite results"}))?;
    }
    case!(
        "native-invalid-fingerprint-port-totp-user-and-missing-key",
        {
            for (index, value) in [
                (6, "SHA256:not-verified"),
                (3, "0"),
                (12, "123456"),
                (0, "bad user"),
            ] {
                let target = suite.output().join(format!("invalid-{index}.json"));
                let mut request = wizard(&target);
                request["answers"][index][1] = json!(value);
                ensure!(terminal(request).await?["code"] == 1);
                ensure!(!target.exists());
            }
            let target = suite.output().join("missing-key.json");
            let mut request = wizard(&target);
            request["args"][4] = json!(suite.output().join("absent-key"));
            ensure!(terminal(request).await?["code"] == 1 && !target.exists());
            Ok(())
        }
    );
    case!(
        "native-wizard-preserves-unrelated-existing-and-default-selection",
        {
            let long = format!("profile-{}", "p".repeat(65));
            for (name, existing, default, prompt, expected) in [
                (
                    "preserve",
                    json!({"other":{"user_name":"other"},"default_account":"other"}),
                    None,
                    false,
                    "other",
                ),
                (
                    "long",
                    json!({long.clone():{"user_name":"other"},"default_account":long.clone()}),
                    None,
                    false,
                    long.as_str(),
                ),
                (
                    "explicit",
                    json!({"other":{"user_name":"other"},"default_account":"other"}),
                    Some("main"),
                    false,
                    "main",
                ),
                (
                    "prompt",
                    json!({"other":{"user_name":"other"}}),
                    None,
                    true,
                    "main",
                ),
            ] {
                let target = suite.output().join(format!("default-{name}.json"));
                write_private(
                    &target,
                    &json!({"unrelated":{"keep":[7,"value"]},"twitterapi_io":existing.clone()}),
                )?;
                let mut request = wizard(&target);
                if let Some(default) = default {
                    request["args"]
                        .as_array_mut()
                        .unwrap()
                        .extend([json!("--default-account"), json!(default)]);
                }
                if prompt {
                    request["answers"]
                        .as_array_mut()
                        .unwrap()
                        .insert(0, json!(["既定にするアカウント設定名: ", "main", false]));
                }
                let out = terminal(request).await?;
                ensure!(out["code"] == 0, "Wizard failed: {}", out["output"]);
                let doc = document(&target)?;
                assert_registered_settings(&doc, &key, &secrets)?;
                ensure!(
                    doc["unrelated"] == json!({"keep":[7,"value"]})
                        && doc["twitterapi_io"]["default_account"] == expected
                );
                for (key, value) in existing.as_object().unwrap() {
                    if key != "default_account" {
                        ensure!(doc["twitterapi_io"][key] == *value);
                    }
                }
            }
            for (index, choice) in ["", "missing", "MAIN"].iter().enumerate() {
                let target = suite.output().join(format!("bad-choice-{index}.json"));
                write_private(
                    &target,
                    &json!({"twitterapi_io":{"other":{"user_name":"other"}}}),
                )?;
                let before = fs::read(&target)?;
                let mut request = wizard(&target);
                request["answers"]
                    .as_array_mut()
                    .unwrap()
                    .insert(0, json!(["既定にするアカウント設定名: ", choice, false]));
                ensure!(terminal(request).await?["code"] == 1 && fs::read(&target)? == before);
            }
            Ok(())
        }
    );
    case!("native-set-default-only-preserves-profile-and-relay", {
        let target = suite.output().join("default-only.json");
        let mut doc = document(&settings)?;
        let long = format!("profile-{}", "p".repeat(65));
        doc["twitterapi_io"][&long] = json!({"user_name":"existing"});
        doc["twitterapi_io"]["DEFAULT_ACCOUNT"] = json!({"user_name":"uppercase"});
        write_private(&target, &doc)?;
        for choice in [long.as_str(), "DEFAULT_ACCOUNT", "main"] {
            cli(
                &[
                    "--settings-path".into(),
                    target.display().to_string(),
                    "--set-default-account".into(),
                    choice.into(),
                ],
                &[],
                0,
                &secrets,
            )
            .await?;
            doc["twitterapi_io"]["default_account"] = json!(choice);
            ensure!(document(&target)? == doc);
        }
        let before = fs::read(&target)?;
        for choice in ["", "missing", "MAIN"] {
            cli(
                &[
                    "--settings-path".into(),
                    target.display().to_string(),
                    "--set-default-account".into(),
                    choice.into(),
                ],
                &[],
                1,
                &secrets,
            )
            .await?;
            ensure!(fs::read(&target)? == before);
        }
        Ok(())
    });
    for residue in [0usize, 2, 4, 6] {
        let case_name = format!("native-settings-publish-exact-name-byte-boundary-{residue}");
        case!(&case_name, {
            use std::os::windows::ffi::OsStrExt;
            let directory = suite
                .output()
                .join(format!("publish-name-boundary-{residue}"));
            fs::create_dir(&directory)?;
            let offset = std::mem::offset_of!(
                windows_sys::Win32::Storage::FileSystem::FILE_RENAME_INFO,
                FileName
            );
            // UTF-16単位で全4境界を選ぶ。日本語とサロゲートペアも元のファイル名へ保存する。
            let target = (0..4)
                .map(|padding| {
                    directory.join(format!("settings-保存-😀{}.json", "x".repeat(padding)))
                })
                .find(|path| (offset + path.as_os_str().encode_wide().count() * 2) % 8 == residue)
                .context("Cannot select requested filename byte boundary")?;
            let expected_name = target.file_name().unwrap().to_os_string();
            let out = terminal(wizard(&target)).await?;
            ensure!(
                out["code"] == 0 && out["answered"] == 14,
                "New settings publication failed at filename byte boundary {residue}"
            );
            let mut expected = document(&target)?;
            assert_registered_settings(&expected, &key, &secrets)?;
            ensure!(
                expected["twitterapi_io"]["default_account"] == "main"
                    && expected["twitterapi_io"]["main"]["api_key"] == secrets[0]
            );
            let entries = fs::read_dir(&directory)?
                .map(|entry| Ok(entry?.file_name()))
                .collect::<Result<Vec<_>>>()?;
            ensure!(
                entries == vec![expected_name.clone()],
                "New settings publication created an unexpected filename or staging entry"
            );
            assert_private_acl(&target)?;
            expected["twitterapi_io"]["other"] = json!({"user_name":"Other"});
            write_private(&target, &expected)?;
            cli(
                &[
                    "--settings-path".into(),
                    target.display().to_string(),
                    "--set-default-account".into(),
                    "other".into(),
                ],
                &[],
                0,
                &secrets,
            )
            .await?;
            expected["twitterapi_io"]["default_account"] = json!("other");
            ensure!(
                document(&target)? == expected,
                "Replacement did not preserve settings under the exact requested filename"
            );
            let entries = fs::read_dir(&directory)?
                .map(|entry| Ok(entry?.file_name()))
                .collect::<Result<Vec<_>>>()?;
            ensure!(
                entries == vec![expected_name],
                "Replacement created an unexpected filename or staging entry"
            );
            assert_private_acl(&target)?;
            Ok(())
        });
    }
    case!(
        "native-check-rejects-invalid-settings-default-and-proxy-readonly",
        {
            for (index, value) in [
                Value::Null,
                json!(12),
                json!({}),
                json!("missing"),
                json!("MAIN"),
            ]
            .into_iter()
            .enumerate()
            {
                let target = suite.output().join(format!("bad-default-{index}.json"));
                let mut doc = document(&settings)?;
                doc["twitterapi_io"]["default_account"] = value;
                write_private(&target, &doc)?;
                let before = fs::read(&target)?;
                cli(
                    &[
                        "--check".into(),
                        "--settings-path".into(),
                        target.display().to_string(),
                    ],
                    &[],
                    1,
                    &secrets,
                )
                .await?;
                ensure!(fs::read(&target)? == before);
            }
            let target = suite.output().join("bad-proxy.json");
            let mut doc = document(&settings)?;
            doc["twitterapi_io"]["main"]["proxy"] = json!("http://example.invalid:3128");
            write_private(&target, &doc)?;
            let before = fs::read(&target)?;
            cli(
                &[
                    "--check".into(),
                    "--settings-path".into(),
                    target.display().to_string(),
                ],
                &[],
                1,
                &secrets,
            )
            .await?;
            ensure!(fs::read(&target)? == before);
            fs::write(&target, "{bad-json dummy-api-secret")?;
            cli(
                &[
                    "--check".into(),
                    "--settings-path".into(),
                    target.display().to_string(),
                ],
                &[],
                1,
                &secrets,
            )
            .await?;
            Ok(())
        }
    );
    case!("native-shared-relay-refused-before-prompt", {
        let target = suite.output().join("shared.json");
        let mut doc = document(&settings)?;
        doc["twitterapi_io"].as_object_mut().unwrap().remove("main");
        doc["twitterapi_io"]
            .as_object_mut()
            .unwrap()
            .remove("default_account");
        write_private(&target, &doc)?;
        let before = fs::read(&target)?;
        let out = terminal(wizard(&target)).await?;
        ensure!(out["code"] == 1 && out["answered"] == 0 && fs::read(&target)? == before);
        Ok(())
    });
    case!("native-private-key-and-settings-acl-refused", {
        acl(&key, true)?;
        let out = cli(
            &[
                "--check".into(),
                "--settings-path".into(),
                settings.display().to_string(),
            ],
            &[],
            1,
            &secrets,
        )
        .await;
        acl(&key, false)?;
        ensure!(out?.contains("ACL_UNSAFE"));
        acl(&settings, true)?;
        let out = cli(
            &[
                "--check".into(),
                "--settings-path".into(),
                settings.display().to_string(),
            ],
            &[],
            1,
            &secrets,
        )
        .await;
        acl(&settings, false)?;
        ensure!(out?.contains("ACL_UNSAFE"));
        Ok(())
    });
    case!(
        "native-junction-ancestor-refused-without-mutating-target",
        {
            let target = suite.output().join("junction-real-directory");
            let link = suite.output().join("junction-link-directory");
            fs::create_dir(&target)?;
            let sentinel = target.join("sentinel.txt");
            fs::write(&sentinel, "junction fixture must remain unchanged")?;
            directory_junction(&link, &target)?;
            let settings = link.join("must-not-be-created.json");
            let key = link.join("must-not-be-created-key");
            for arguments in [
                vec![
                    "--check".into(),
                    "--settings-path".into(),
                    settings.display().to_string(),
                ],
                vec![
                    "--set-default-account".into(),
                    "main".into(),
                    "--settings-path".into(),
                    settings.display().to_string(),
                ],
                vec![
                    "--prepare-key".into(),
                    "--private-key-path".into(),
                    key.display().to_string(),
                ],
            ] {
                ensure!(
                    cli(&arguments, &[], 1, &secrets)
                        .await?
                        .contains("PATH_INVALID"),
                    "Native setup did not reject an actual junction ancestor"
                );
            }
            ensure!(
                fs::read_to_string(&sentinel)? == "junction fixture must remain unchanged"
                    && fs::read_dir(&target)?.count() == 1,
                "Rejected junction path modified its target"
            );
            suite.write_json("protected-path-junction-coverage.json",&json!({"junction":"verified","requiresSymlinkPrivilege":false,"checkedOperations":["check","set-default-account","prepare-key"]}))?;
            Ok(())
        }
    );
    let link = suite.output().join("settings-link.json");
    match std::os::windows::fs::symlink_file(&settings, &link) {
        Ok(()) => {
            case!("native-settings-symlink-refused", {
                ensure!(
                    cli(
                        &[
                            "--check".into(),
                            "--settings-path".into(),
                            link.display().to_string()
                        ],
                        &[],
                        1,
                        &secrets
                    )
                    .await?
                    .contains("PATH_INVALID")
                );
                suite.write_json(
                    "protected-path-link-coverage.json",
                    &json!({"symlink":"verified"}),
                )?;
                Ok(())
            });
        }
        Err(error)
            if error.kind() == std::io::ErrorKind::PermissionDenied
                || error.raw_os_error() == Some(1314) =>
        {
            suite.write_json("protected-path-link-coverage.json",&json!({"symlink":"notRun","reason":"Creating a Windows symlink requires Developer Mode or SeCreateSymbolicLinkPrivilege; link rejection was not exercised","osError":error.raw_os_error()}))?;
        }
        Err(error) => return Err(error.into()),
    }
    case!(
        "native-settings-source-explicit-environment-and-userprofile",
        {
            let local = suite.output().join("selection-local");
            let home = suite.output().join("selection-home");
            let portable = local.join("Kagayoi/Plumivox/settings.json");
            let fallback = home.join("AppData/Local/Kagayoi/Plumivox/settings.json");
            for target in [&portable, &fallback] {
                fs::create_dir_all(target.parent().unwrap())?;
                write_private(target, &document(&settings)?)?;
            }
            let env = vec![
                ("LOCALAPPDATA".into(), local.display().to_string()),
                ("USERPROFILE".into(), home.display().to_string()),
            ];
            cli(&["--check".into()], &env, 0, &secrets).await?;
            let invalid = suite.output().join("invalid-selected.json");
            write_private(&invalid, &json!({}))?;
            let mut selected = env.clone();
            selected.push((
                "PLUMIVOX_SECRETS_FILE".into(),
                invalid.display().to_string(),
            ));
            cli(&["--check".into()], &selected, 1, &secrets).await?;
            // 既存契約では環境変数が明示パスより優先する。
            cli(
                &[
                    "--check".into(),
                    "--settings-path".into(),
                    settings.display().to_string(),
                ],
                &selected,
                1,
                &secrets,
            )
            .await?;
            cli(
                &[
                    "--check".into(),
                    "--settings-path".into(),
                    settings.display().to_string(),
                ],
                &env,
                0,
                &secrets,
            )
            .await?;
            let fallback_env = vec![
                ("LOCALAPPDATA".into(), "".into()),
                ("USERPROFILE".into(), home.display().to_string()),
            ];
            cli(&["--check".into()], &fallback_env, 0, &secrets).await?;
            cli(&["--prepare-key".into()], &fallback_env, 0, &secrets).await?;
            ensure!(
                home.join("AppData/Local/Kagayoi/Plumivox/keys/plumivox_ed25519")
                    .is_file()
            );
            Ok(())
        }
    );
    let root = suite
        .output()
        .join("codex-protected-directory-with-long-name");
    // Codex専用ではないため、Claude Codeの設定フォルダー（既定名と環境変数）も同様に拒否する。
    let claude_root = suite
        .output()
        .join("claude-protected-directory-with-long-name");
    let home = suite.output().join("fixture-user-profile");
    fs::create_dir(&root)?;
    fs::create_dir(&claude_root)?;
    fs::create_dir_all(home.join(".codex"))?;
    fs::create_dir_all(home.join(".claude"))?;
    let env = vec![
        ("CODEX_HOME".into(), root.display().to_string()),
        (
            "CLAUDE_CONFIG_DIR".into(),
            claude_root.display().to_string(),
        ),
        ("USERPROFILE".into(), home.display().to_string()),
    ];
    case!("native-protected-path-dot-components-refused", {
        // Path::components()が消す前の実文字列として、禁止rootより前に `\.` を置く。
        let dotted = PathBuf::from(format!(
            "{}\\.\\{}",
            root.parent().unwrap().display(),
            root.file_name().unwrap().to_string_lossy()
        ));
        let dotted_home = PathBuf::from(format!("{}\\.\\.codex", home.display()));
        let dotted_claude = PathBuf::from(format!(
            "{}\\.\\{}",
            claude_root.parent().unwrap().display(),
            claude_root.file_name().unwrap().to_string_lossy()
        ));
        let dotted_claude_home = PathBuf::from(format!("{}\\.\\.claude", home.display()));
        for candidate in [dotted, dotted_home, dotted_claude, dotted_claude_home] {
            let target = candidate.join("must-not-be-created.json");
            ensure!(
                cli(
                    &[
                        "--check".into(),
                        "--settings-path".into(),
                        target.display().to_string()
                    ],
                    &env,
                    1,
                    &secrets
                )
                .await?
                .contains("PATH_INVALID")
            );
            let key = candidate.join("must-not-be-created-key");
            ensure!(
                cli(
                    &[
                        "--prepare-key".into(),
                        "--private-key-path".into(),
                        key.display().to_string()
                    ],
                    &env,
                    1,
                    &secrets
                )
                .await?
                .contains("PATH_INVALID")
            );
        }
        ensure!(
            fs::read_dir(&root)?.next().is_none()
                && fs::read_dir(&claude_root)?.next().is_none()
                && fs::read_dir(home.join(".codex"))?.next().is_none()
                && fs::read_dir(home.join(".claude"))?.next().is_none(),
            "Rejected protected path created a file or lock"
        );
        Ok(())
    });
    if let Some(alias) = short_alias(&root)? {
        case!("native-protected-path-short-alias-refused", {
            let target = alias.join("must-not-be-created.json");
            ensure!(
                cli(
                    &[
                        "--check".into(),
                        "--settings-path".into(),
                        target.display().to_string()
                    ],
                    &env,
                    1,
                    &secrets
                )
                .await?
                .contains("PATH_INVALID")
            );
            let key = alias.join("must-not-be-created-key");
            ensure!(
                cli(
                    &[
                        "--prepare-key".into(),
                        "--private-key-path".into(),
                        key.display().to_string()
                    ],
                    &env,
                    1,
                    &secrets
                )
                .await?
                .contains("PATH_INVALID")
            );
            ensure!(
                fs::read_dir(&root)?.next().is_none(),
                "Rejected short alias created a file or lock"
            );
            suite.write_json(
                "protected-path-alias-coverage.json",
                &json!({"shortAlias":"verified"}),
            )?;
            Ok(())
        });
    } else {
        suite.write_json("protected-path-alias-coverage.json",&json!({"shortAlias":"notRun","reason":"Fixture root has no usable distinct 8.3 alias; short-name rejection was not exercised and is excluded from suite results"}))?;
    }
    case!("native-default-change-hardens-owned-inherited-settings", {
        let target = wide_parent.join("legacy-inherited-settings.json");
        let mut doc = document(&settings)?;
        doc["twitterapi_io"]["other"] = json!({"user_name":"Other"});
        fs::write(&target, serde_json::to_vec(&doc)?)?;
        assert_inherited_public_read(&target)?;
        cli(
            &[
                "--check".into(),
                "--settings-path".into(),
                target.display().to_string(),
            ],
            &[],
            1,
            &secrets,
        )
        .await?;
        cli(
            &[
                "--settings-path".into(),
                target.display().to_string(),
                "--set-default-account".into(),
                "other".into(),
            ],
            &[],
            0,
            &secrets,
        )
        .await?;
        doc["twitterapi_io"]["default_account"] = json!("other");
        ensure!(document(&target)? == doc);
        assert_private_acl(&target)?;
        Ok(())
    });
    Ok(())
}
