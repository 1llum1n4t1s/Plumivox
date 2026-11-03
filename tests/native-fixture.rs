use std::{path::PathBuf, time::Duration};

#[cfg(windows)]
struct NonInheritedStdio(Vec<(windows_sys::Win32::Foundation::HANDLE, u32)>);

#[cfg(windows)]
impl NonInheritedStdio {
    fn new() -> std::io::Result<Self> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::{
            GetHandleInformation, HANDLE_FLAG_INHERIT, SetHandleInformation,
        };
        let mut guard = Self(Vec::new());
        for handle in [
            std::io::stdout().as_raw_handle(),
            std::io::stdin().as_raw_handle(),
        ] {
            let mut flags = 0;
            unsafe {
                if GetHandleInformation(handle, &mut flags) == 0 {
                    return Err(std::io::Error::last_os_error());
                }
                guard.0.push((handle, flags));
                if SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) == 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
        }
        Ok(guard)
    }

    fn restore(&mut self) -> std::io::Result<()> {
        use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};
        let mut failed = false;
        for (handle, flags) in &self.0 {
            if unsafe {
                SetHandleInformation(*handle, HANDLE_FLAG_INHERIT, *flags & HANDLE_FLAG_INHERIT)
            } == 0
            {
                failed = true;
            }
        }
        if failed {
            return Err(std::io::Error::last_os_error());
        }
        self.0.clear();
        Ok(())
    }
}

#[cfg(windows)]
impl Drop for NonInheritedStdio {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

// 任意APIや疑似故障の注入は配布バイナリへ組み込まない。
#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "--hold-stderr") {
        let milliseconds = args.get(1).and_then(|arg| arg.parse::<u64>().ok());
        if args.len() != 2 || !milliseconds.is_some_and(|value| (100..=2500).contains(&value)) {
            std::process::exit(2);
        }
        tokio::time::sleep(Duration::from_millis(milliseconds.unwrap())).await;
        return;
    }
    if args.as_slice() == ["--proxy-daemon"] {
        #[cfg(unix)]
        unsafe {
            libc::umask(0o077);
        }
        use std::io::Read;
        let mut bytes = Vec::new();
        if std::io::stdin().take(8193).read_to_end(&mut bytes).is_err() || bytes.len() > 8192 {
            std::process::exit(2);
        }
        let Ok(bootstrap) = serde_json::from_slice(&bytes) else {
            std::process::exit(2)
        };
        if plumivox::proxy::run_daemon(bootstrap).await.is_err() {
            std::process::exit(1);
        }
        return;
    }
    if args.len() < 3 {
        std::process::exit(2);
    }
    let mode = args.get(4).filter(|value| value.starts_with("--"));
    let tls_mode = mode.is_some_and(|mode| {
        matches!(
            mode.as_str(),
            "--tls-trusted-fixture" | "--tls-untrusted-fixture"
        )
    });
    let Ok(url) = url::Url::parse(&args[0]) else {
        std::process::exit(2)
    };
    if url.scheme() != if tls_mode { "https" } else { "http" }
        || url.host_str() != Some("127.0.0.1")
        || url.port().is_none()
        || url.path() != "/"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        std::process::exit(2);
    }
    let timeout = args
        .get(3)
        .and_then(|arg| arg.parse::<u64>().ok())
        .unwrap_or(250);
    if !(250..=15000).contains(&timeout) {
        std::process::exit(2);
    }
    if tls_mode {
        if plumivox::client::configure_tls_fixture(
            mode.is_some_and(|mode| mode == "--tls-trusted-fixture"),
        )
        .is_err()
        {
            std::process::exit(2);
        }
    } else if let Some(mode) = mode {
        plumivox::server::configure_fixture_mode(mode.clone());
    }
    let options = plumivox::Options {
        api_base_url: args[0].clone(),
        secrets_path: Some(PathBuf::from(&args[1])),
        state_directory: Some(PathBuf::from(&args[2])),
        profile: args
            .get(4)
            .filter(|value| !value.starts_with("--"))
            .cloned(),
        timeout: Duration::from_millis(timeout),
        image_timeout: Duration::from_millis(
            if mode.is_some_and(|mode| mode == "--slow-image-upload") {
                1500
            } else {
                timeout
            },
        ),
    };
    if plumivox::run_stdio(options).await.is_err() {
        std::process::exit(1);
    }
    match mode.map(String::as_str) {
        Some("--stderr-after-eof") => {
            println!(
                "{}",
                serde_json::json!({"jsonrpc":"2.0","method":"notifications/message",
                "params":{"level":"info","data":"synthetic-shutdown"}})
            );
            eprintln!("synthetic-transport-tail");
        }
        Some("--stdout-invalid-after-eof") => {
            use std::io::Write;
            let mut stdout = std::io::stdout().lock();
            if stdout
                .write_all(&[0xff, b'\n'])
                .and_then(|_| stdout.flush())
                .is_err()
            {
                std::process::exit(1);
            }
        }
        Some("--stdout-nonmcp-after-eof") => {
            println!("{}", serde_json::json!({"unrelated":"synthetic"}))
        }
        Some("--stdout-unknown-id-after-eof") => println!(
            "{}",
            serde_json::json!({"jsonrpc":"2.0","id":999999,"result":{}})
        ),
        Some("--stderr-hold-after-eof") => {
            use std::process::{Command, Stdio};
            let Ok(executable) = std::env::current_exe() else {
                std::process::exit(1)
            };
            // WindowsのCreateProcessは明示したstdio以外の継承可能handleも複製する。
            // 診断子が保持するのをstderrだけに限定し、起動後は元のflagへ戻す。
            #[cfg(windows)]
            let mut inheritance = match NonInheritedStdio::new() {
                Ok(guard) => guard,
                Err(_) => std::process::exit(1),
            };
            let launched = Command::new(executable)
                .args(["--hold-stderr", "1500"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn();
            #[cfg(windows)]
            let restored = inheritance.restore().is_ok();
            #[cfg(not(windows))]
            let restored = true;
            if launched.is_err() || !restored {
                std::process::exit(1);
            }
        }
        _ => {}
    }
}
