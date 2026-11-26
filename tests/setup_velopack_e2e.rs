//! Setup（Velopack）導入版の隔離E2E（Windows）。`cargo xtask test-setup-velopack-e2e` から起動する。
//!
//! xtask がテスト用 packId（`PlumivoxVelopackTest`）の 9.0.0・9.0.1 を未署名で梱包し、このテストが
//! 本物の `Setup.exe --silent --installto <出力先配下>` で導入して、登録・自動更新・適用・解除を確かめる。
//!
//! 本物の環境に触れないための隔離:
//! - 子プロセスの `LOCALAPPDATA`・`APPDATA`・`USERPROFILE`・`HOME`・`CODEX_HOME`・`CLAUDE_CONFIG_DIR`・
//!   `TEMP`/`TMP`・`PLUMIVOX_SECRETS_FILE` を出力先の配下へ向ける。
//! - `PATH` はスタブ（`claude.exe`・`codex.exe` の代役）のフォルダーと System32 だけにする。スタブが欠けても
//!   本物の CLI に落ちない。
//! - 導入先と差し替えた `LOCALAPPDATA` は兄弟のフォルダーにする（入れ子だと保護判定とアンインストールが
//!   データフォルダーを巻き込む）。
//! - HKCU の Run 値と Uninstall の項目はテスト用 packId の名前だけが作られる。終わりに必ずアンインストールし、
//!   本番の packId（`PlumivoxApp`・`Plumivox`）の値と、本物の Claude Code・Codex の `plumivox` 登録が
//!   前後で変わらないことを確かめる。
//! - 管理プロキシは `tests/proxy_support.rs` の偽 SSH（127.0.0.1）だけへ接続する。
#[cfg(windows)]
#[allow(dead_code)]
mod proxy_support;
// support/mod.rs 自身が dead_code を許可している。
mod support;

#[cfg(not(windows))]
#[test]
fn setup_velopack_e2e_requires_windows() {
    panic!(
        "Setup（Velopack）E2EはWindowsで cargo xtask test-setup-velopack-e2e から実行してください"
    );
}

#[cfg(windows)]
mod windows_e2e {
    use super::{proxy_support::Harness, support};
    use anyhow::{Context, bail, ensure};
    use serde_json::{Value, json};
    use std::{
        collections::BTreeMap,
        fs,
        io::Read,
        os::windows::process::CommandExt,
        path::{Path, PathBuf},
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    use support::{McpClient, Result, Suite};

    const PACK_ID: &str = "PlumivoxVelopackTest";
    const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    const UNINSTALL_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall";
    /// 本番の packId（新しい `PlumivoxApp` と、衝突を避けた旧名 `Plumivox`）。テストの前後で不変を確かめる。
    const PRODUCTION_IDS: [&str; 2] = ["PlumivoxApp", "Plumivox"];
    /// 共有中継 daemon の待機時間。MCP を閉じた直後の適用が busy になり、その後に終わる長さにする。
    const DAEMON_IDLE_MS: &str = "8000";

    fn required(name: &str) -> Result<PathBuf> {
        let value = std::env::var_os(name)
            .filter(|value| !value.is_empty())
            .with_context(|| {
                format!(
                    "{name} がありません。cargo xtask test-setup-velopack-e2e から起動してください"
                )
            })?;
        let path = PathBuf::from(value);
        ensure!(path.is_absolute(), "{name} must be absolute");
        Ok(path)
    }

    /// 比較用の正規化（区切りを / に揃え、小文字にする）。
    fn normal(text: &str) -> String {
        text.replace('\\', "/").to_ascii_lowercase()
    }

    /// 子プロセスを上限時間つきで実行し、終了コード・標準出力・標準エラーを返す。窓は出さない。
    fn run(mut command: Command, timeout: Duration) -> Result<(i32, String, String)> {
        command
            .creation_flags(0x0800_0000)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().context("cannot start a child process")?;
        let mut stdout = child.stdout.take().context("stdout")?;
        let mut stderr = child.stderr.take().context("stderr")?;
        let out = std::thread::spawn(move || {
            let mut text = Vec::new();
            let _ = stdout.read_to_end(&mut text);
            text
        });
        let err = std::thread::spawn(move || {
            let mut text = Vec::new();
            let _ = stderr.read_to_end(&mut text);
            text
        });
        let deadline = Instant::now() + timeout;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                bail!("child process timed out: {command:?}");
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let stdout = String::from_utf8_lossy(&out.join().unwrap_or_default()).into_owned();
        let stderr = String::from_utf8_lossy(&err.join().unwrap_or_default()).into_owned();
        Ok((status.code().unwrap_or(-1), stdout, stderr))
    }

    /// `reg query` の読み取り（読み取り専用）。値が無ければ None。
    fn reg_query(key: &str, value: Option<&str>) -> Result<Option<String>> {
        let system = std::env::var_os("SystemRoot").context("SystemRoot")?;
        let mut command = Command::new(Path::new(&system).join("System32").join("reg.exe"));
        command.arg("query").arg(key);
        match value {
            Some(name) => {
                command.arg("/v").arg(name);
            }
            None => {
                command.arg("/s");
            }
        }
        let (code, stdout, _) = run(command, Duration::from_secs(30))?;
        Ok((code == 0).then_some(stdout))
    }

    /// Run 値の中身（`REG_SZ` の後ろ）。無ければ None。
    fn run_value(name: &str) -> Result<Option<String>> {
        Ok(reg_query(RUN_KEY, Some(name))?.and_then(|text| {
            text.lines()
                .find_map(|line| line.split_once("REG_SZ").map(|(_, v)| v.trim().to_string()))
        }))
    }

    /// テストの前後で変わってはいけない、本物の環境の値。
    fn production_snapshot() -> Result<Value> {
        let mut snapshot = BTreeMap::new();
        for id in PRODUCTION_IDS {
            snapshot.insert(
                format!("run:{id} Update"),
                json!(run_value(&format!("{id} Update"))?),
            );
            snapshot.insert(
                format!("uninstall:{id}"),
                json!(reg_query(&format!(r"{UNINSTALL_KEY}\{id}"), None)?),
            );
        }
        // 本物の Claude Code（利用者単位）と Codex の plumivox 登録。ファイル全体は実行中のセッションが
        // 書き換えるため、plumivox の項目だけを比べる。
        let home = dirs::home_dir().context("home")?;
        let claude = std::env::var_os("CLAUDE_CONFIG_DIR")
            .filter(|v| !v.is_empty())
            .map(|dir| PathBuf::from(dir).join(".claude.json"))
            .unwrap_or_else(|| home.join(".claude.json"));
        let claude_entry = fs::read(&claude)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .map(|value| value["mcpServers"]["plumivox"].clone());
        snapshot.insert("claude:mcpServers.plumivox".into(), json!(claude_entry));
        let codex = std::env::var_os("CODEX_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"))
            .join("config.toml");
        let section = fs::read_to_string(&codex).ok().map(|text| {
            let mut inside = false;
            text.lines()
                .filter(|line| {
                    let trimmed = line.trim();
                    if trimmed.starts_with('[') {
                        inside = trimmed.starts_with("[mcp_servers.plumivox");
                    }
                    inside
                })
                .collect::<Vec<_>>()
                .join("\n")
        });
        snapshot.insert("codex:mcp_servers.plumivox".into(), json!(section));
        // テスト用 packId が本物の LOCALAPPDATA・スタートメニューへ漏れていないこと。
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            snapshot.insert(
                format!("localappdata:{PACK_ID}"),
                json!(Path::new(&local).join(PACK_ID).exists()),
            );
        }
        if let Some(roaming) = std::env::var_os("APPDATA") {
            snapshot.insert(
                "startmenu:Plumivox Velopack Test".into(),
                json!(
                    Path::new(&roaming)
                        .join(r"Microsoft\Windows\Start Menu\Programs\Plumivox Velopack Test.lnk")
                        .exists()
                ),
            );
        }
        Ok(json!(snapshot))
    }

    /// 隔離した環境（出力先の配下）と、その中の各パス。
    struct World {
        root: PathBuf,
        stub: PathBuf,
        feed: PathBuf,
        data: PathBuf,
        legacy: PathBuf,
        vars: Vec<(String, String)>,
    }

    impl World {
        fn new(root: &Path, stub_exe: &Path, secrets: &Path) -> Result<Self> {
            let env = root.join("env");
            let stub = root.join("stub");
            let feed = root.join("feed");
            for dir in [
                "localappdata",
                "appdata",
                "home",
                "codex-home",
                "claude-config",
                "temp",
            ] {
                fs::create_dir_all(env.join(dir))?;
            }
            fs::create_dir_all(&stub)?;
            fs::create_dir_all(&feed)?;
            for name in ["claude.exe", "codex.exe"] {
                fs::copy(stub_exe, stub.join(name))?;
            }
            let system = std::env::var("SystemRoot").context("SystemRoot")?;
            let path = |p: PathBuf| p.to_string_lossy().into_owned();
            let vars = vec![
                ("LOCALAPPDATA".into(), path(env.join("localappdata"))),
                ("APPDATA".into(), path(env.join("appdata"))),
                ("USERPROFILE".into(), path(env.join("home"))),
                ("HOME".into(), path(env.join("home"))),
                ("CODEX_HOME".into(), path(env.join("codex-home"))),
                ("CLAUDE_CONFIG_DIR".into(), path(env.join("claude-config"))),
                ("TEMP".into(), path(env.join("temp"))),
                ("TMP".into(), path(env.join("temp"))),
                // スタブと System32 だけ。本物の claude.exe・codex.exe は見つからない。
                (
                    "PATH".into(),
                    format!("{};{system}\\System32;{system}", stub.display()),
                ),
                // Toolport の判定は隔離した registry だけを見る（本物の Toolport・Conduit の設定を読まない）。
                (
                    "TOOLPORT_REGISTRY".into(),
                    path(env.join("appdata").join("Toolport").join("registry.json")),
                ),
                ("PLUMIVOX_SECRETS_FILE".into(), path(secrets.to_path_buf())),
                ("PLUMIVOX_CLI_STUB_DIR".into(), path(stub.clone())),
                ("PLUMIVOX_UPDATE_FEED_DIR".into(), path(feed.clone())),
                ("PLUMIVOX_PROXY_TEST_IDLE_MS".into(), DAEMON_IDLE_MS.into()),
                ("PLUMIVOX_PROXY_TEST_STARTUP_MS".into(), "5000".into()),
            ];
            Ok(Self {
                data: env.join("localappdata").join("Kagayoi").join("Plumivox"),
                legacy: root.join("legacy-plugin").join("dist").join("plumivox.exe"),
                root: root.to_path_buf(),
                stub,
                feed,
                vars,
            })
        }

        fn command(&self, program: &Path) -> Command {
            let mut command = Command::new(program);
            for key in [
                "PLUMIVOX_STATE_DIR",
                "PLUMIVOX_DEFAULT_ACCOUNT",
                "PLUMIVOX_TEST_HOOKS",
                "PLUMIVOX_UPDATE_ALLOW_UNSIGNED",
                "PLUMIVOX_TEST_PACK_VERSION",
                "VELOPACK_FIRSTRUN",
                "TOOLPORT_DATA_DIR",
                "TOOLPORT_PROFILE",
                "CONDUIT_REGISTRY",
                "CONDUIT_DATA_DIR",
                "CONDUIT_PROFILE",
            ] {
                command.env_remove(key);
            }
            command.envs(self.vars.iter().map(|(k, v)| (k, v)));
            command
        }

        /// [`Self::command`] と同じ環境（このプロセスの環境から除く変数を外し、隔離の変数で上書き）を一覧で返す。
        fn environment(&self) -> Vec<(String, String)> {
            let removed = [
                "PLUMIVOX_STATE_DIR",
                "PLUMIVOX_DEFAULT_ACCOUNT",
                "PLUMIVOX_TEST_HOOKS",
                "PLUMIVOX_UPDATE_ALLOW_UNSIGNED",
                "PLUMIVOX_TEST_PACK_VERSION",
                "VELOPACK_FIRSTRUN",
                "TOOLPORT_DATA_DIR",
                "TOOLPORT_PROFILE",
                "CONDUIT_REGISTRY",
                "CONDUIT_DATA_DIR",
                "CONDUIT_PROFILE",
            ];
            let mut map: BTreeMap<String, (String, String)> = BTreeMap::new();
            for (key, value) in std::env::vars_os() {
                let (Some(key), Some(value)) = (key.to_str(), value.to_str()) else {
                    continue;
                };
                if !removed.iter().any(|name| name.eq_ignore_ascii_case(key)) {
                    map.insert(key.to_ascii_uppercase(), (key.into(), value.into()));
                }
            }
            for (key, value) in &self.vars {
                map.insert(key.to_ascii_uppercase(), (key.clone(), value.clone()));
            }
            map.into_values().collect()
        }

        fn install_dir(&self, cycle: &str) -> PathBuf {
            self.root.join(format!("install-{cycle}"))
        }

        fn exe(install: &Path) -> PathBuf {
            install.join("current").join("plumivox.exe")
        }

        /// 本物の Setup.exe で導入する。`--silent` の Setup は導入後にアプリを起動しない（Velopack 1.2.161 の
        /// Setup のログで確認済み）ため、初回起動の案内・登録のやり直しは走らない。なお Setup の
        /// `-- <EXE_ARGS>` は 1.2.161 では引数解析の panic で固まるため使わない。
        fn install(&self, setup: &Path, install: &Path) -> Result<()> {
            let mut command = self.command(setup);
            command.arg("--silent").arg("--installto").arg(install);
            let (code, stdout, stderr) = run(command, Duration::from_secs(180))?;
            ensure!(code == 0, "Setup failed ({code}): {stdout} {stderr}");
            ensure!(Self::exe(install).is_file(), "the app was not installed");
            Ok(())
        }

        fn uninstall(&self, install: &Path) -> Result<()> {
            let mut command = self.command(&install.join("Update.exe"));
            command.args(["uninstall", "--silent"]);
            let (code, stdout, stderr) = run(command, Duration::from_secs(180))?;
            ensure!(code == 0, "uninstall failed ({code}): {stdout} {stderr}");
            Ok(())
        }

        /// `--update-apply`。`allow_unsigned` はテスト用の未署名許可（update-feed-test 機能のビルドだけが読む）。
        fn update_apply(&self, install: &Path, allow_unsigned: bool) -> Result<(i32, Value)> {
            let mut command = self.command(&Self::exe(install));
            command.arg("--update-apply");
            if allow_unsigned {
                command.env("PLUMIVOX_UPDATE_ALLOW_UNSIGNED", "1");
            }
            let (code, stdout, stderr) = run(command, Duration::from_secs(120))?;
            let value = serde_json::from_str(stdout.trim()).with_context(|| {
                format!("--update-apply printed non-JSON ({code}): {stdout} {stderr}")
            })?;
            Ok((code, value))
        }

        fn version(&self, install: &Path) -> Result<String> {
            let mut command = self.command(&Self::exe(install));
            command.arg("--version");
            let (code, stdout, _) = run(command, Duration::from_secs(30))?;
            ensure!(code == 0, "--version failed");
            Ok(stdout.trim().to_string())
        }

        async fn mcp(&self, install: &Path) -> Result<McpClient> {
            McpClient::spawn_command(&Self::exe(install), &["--stdio".to_string()], &self.vars)
                .await
        }

        fn calls(&self) -> Result<Vec<Value>> {
            let path = self.stub.join("calls.jsonl");
            let text = fs::read_to_string(&path).unwrap_or_default();
            text.lines()
                .filter(|line| !line.trim().is_empty())
                .map(|line| Ok(serde_json::from_str(line)?))
                .collect()
        }

        /// 記録の要約（`claude mcp get plumivox` のような1行の文字列）。
        fn call_lines(&self, from: usize) -> Result<Vec<String>> {
            Ok(self
                .calls()?
                .into_iter()
                .skip(from)
                .map(|call| {
                    let args = call["args"]
                        .as_array()
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join(" ")
                        })
                        .unwrap_or_default();
                    format!("{} {args}", call["tool"].as_str().unwrap_or("?"))
                })
                .collect())
        }

        fn registry(&self, tool: &str) -> Value {
            fs::read(self.stub.join(format!("{tool}-mcp.json")))
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .unwrap_or_else(|| json!({}))
        }

        fn set_registry(&self, tool: &str, command: Option<&Path>) -> Result<()> {
            let value = match command {
                Some(command) => {
                    json!({"plumivox": {"command": command.to_string_lossy(), "args": ["--stdio"]}})
                }
                None => json!({}),
            };
            fs::write(
                self.stub.join(format!("{tool}-mcp.json")),
                serde_json::to_vec(&value)?,
            )?;
            Ok(())
        }

        fn set_faults(&self, faults: Value) -> Result<()> {
            fs::write(self.stub.join("faults.json"), serde_json::to_vec(&faults)?)?;
            Ok(())
        }

        fn state_dir(&self) -> PathBuf {
            self.data.join("app-update")
        }

        fn registration(&self) -> Value {
            fs::read(self.state_dir().join("setup-registration.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .unwrap_or(Value::Null)
        }

        fn log(&self) -> String {
            fs::read_to_string(self.state_dir().join("app-update.log")).unwrap_or_default()
        }

        /// 登録先がこの実行ファイルを指しているか。
        fn points_at(entry: &Value, exe: &Path) -> bool {
            normal(entry["plumivox"]["command"].as_str().unwrap_or(""))
                == normal(&exe.to_string_lossy())
                && entry["plumivox"]["args"] == json!(["--stdio"])
        }
    }

    /// 導入先の配下の実行ファイルから動いているプロセスの ID（アプリの判定と同じ前方一致）。
    fn processes_under(root: &Path) -> Vec<u32> {
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
        let mut prefix = normal(&root.to_string_lossy());
        if !prefix.ends_with('/') {
            prefix.push('/');
        }
        let mut found = Vec::new();
        // SAFETY: Win32 の列挙 API。ハンドルは必ず閉じる。
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return found;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut more = Process32FirstW(snapshot, &mut entry) != 0;
            while more {
                let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID);
                if !handle.is_null() {
                    let mut buffer = vec![0u16; 32 * 1024];
                    let mut length = buffer.len() as u32;
                    if QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length) != 0
                    {
                        let image = OsString::from_wide(&buffer[..length as usize]);
                        let image = normal(&image.to_string_lossy());
                        if image.trim_start_matches("//?/").starts_with(&prefix) {
                            found.push(entry.th32ProcessID);
                        }
                    }
                    CloseHandle(handle);
                }
                more = Process32NextW(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
        }
        found
    }

    /// 標準ハンドルを渡さず（STARTF_USESTDHANDLES なし・ハンドル継承なし）、窓も出さずに起動し、終了コードを返す。
    /// Explorer がログオン時に Run 値を起動するときと同じ形にするため（conhost --headless の検証用）。
    fn spawn_without_std_handles(
        command_line: &str,
        environment: &[(String, String)],
        timeout: Duration,
    ) -> Result<i32> {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, WAIT_OBJECT_0},
            System::Threading::{
                CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, GetExitCodeProcess,
                PROCESS_INFORMATION, STARTUPINFOW, TerminateProcess, WaitForSingleObject,
            },
        };
        let mut line: Vec<u16> = command_line.encode_utf16().chain(Some(0)).collect();
        // 環境ブロック: "KEY=VALUE\0" の並び（大小を無視した名前順）と、最後の追加の "\0"。
        let mut block: Vec<u16> = Vec::new();
        for (key, value) in environment {
            block.extend(format!("{key}={value}").encode_utf16());
            block.push(0);
        }
        block.push(0);
        // SAFETY: 構造体はゼロ初期化して大きさを設定し、得たハンドルは必ず閉じる。
        unsafe {
            let mut startup: STARTUPINFOW = std::mem::zeroed();
            startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
            let mut info: PROCESS_INFORMATION = std::mem::zeroed();
            ensure!(
                CreateProcessW(
                    std::ptr::null(),
                    line.as_mut_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
                    block.as_ptr().cast(),
                    std::ptr::null(),
                    &startup,
                    &mut info,
                ) != 0,
                "cannot start the Run command: {}",
                std::io::Error::last_os_error()
            );
            CloseHandle(info.hThread);
            let waited = WaitForSingleObject(info.hProcess, timeout.as_millis() as u32);
            if waited != WAIT_OBJECT_0 {
                TerminateProcess(info.hProcess, 1);
                CloseHandle(info.hProcess);
                bail!("the Run command timed out");
            }
            let mut code = 0u32;
            GetExitCodeProcess(info.hProcess, &mut code);
            CloseHandle(info.hProcess);
            Ok(code as i32)
        }
    }

    async fn wait_for<F: FnMut() -> Result<bool>>(
        timeout: Duration,
        what: &str,
        mut condition: F,
    ) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            if condition()? {
                return Ok(());
            }
            ensure!(Instant::now() < deadline, "timed out waiting for {what}");
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    fn copy_feed(from: &Path, to: &Path) -> Result<()> {
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                fs::copy(entry.path(), to.join(entry.file_name()))?;
            }
        }
        Ok(())
    }

    /// 終わりに、残っているテスト用の導入を本物のアンインストーラーで外す（失敗経路でも Run 値を残さない）。
    fn uninstall_leftovers(world: &World, installs: &[PathBuf]) -> Vec<String> {
        let mut notes = Vec::new();
        for install in installs {
            if install.join("Update.exe").is_file() && World::exe(install).is_file() {
                notes.push(match world.uninstall(install) {
                    Ok(()) => format!("uninstalled leftover {}", install.display()),
                    Err(error) => {
                        format!("leftover uninstall failed {}: {error:#}", install.display())
                    }
                });
            }
        }
        notes
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn setup_velopack_isolated_e2e() -> Result<()> {
        let output = required("PLUMIVOX_VELOPACK_E2E_OUTPUT_DIR")?;
        let setup = required("PLUMIVOX_VELOPACK_E2E_SETUP")?;
        let feed_v1 = required("PLUMIVOX_VELOPACK_E2E_FEED_V1")?;
        let feed_v2 = required("PLUMIVOX_VELOPACK_E2E_FEED_V2")?;
        let stub = required("PLUMIVOX_VELOPACK_E2E_STUB")?;
        // 前回のテストの Run 値が残っていたら、本物の導入と区別できないので始めない。
        ensure!(
            run_value(&format!("{PACK_ID} Update"))?.is_none()
                && reg_query(&format!(r"{UNINSTALL_KEY}\{PACK_ID}"), None)?.is_none(),
            "a previous {PACK_ID} Run value or Uninstall entry remains; remove it first"
        );
        let before = production_snapshot()?;
        let mut suite = Suite::new_at(output, "setup-velopack")?;
        let mut harness = Harness::new(suite.output()).await?;
        for secret in &harness.secrets {
            suite.add_secret(secret.clone());
        }
        let settings = harness.settings("velopack", json!({})).await?;
        let world = World::new(suite.output(), &stub, &settings.file)?;
        copy_feed(&feed_v1, &world.feed)?;
        let first = world.install_dir("1");
        let second = world.install_dir("2");
        let mut evidence = BTreeMap::<String, Value>::new();

        macro_rules! case {
            ($name:expr, $body:block) => {{
                let result: Result<()> = async { $body }.await;
                suite.record($name, result)
            }};
        }

        // ───── 1回目の導入: 新しい登録 ─────
        let first_installed = case!(
            "(1) install registers get->add on both CLIs, Run value and state",
            {
                world.install(&setup, &first)?;
                let exe = World::exe(&first);
                let exe_text = exe.to_string_lossy().into_owned();
                let lines = world.call_lines(0)?;
                evidence.insert("cycle1-install-calls".into(), json!(lines));
                let expected = [
                    "claude mcp get plumivox".to_string(),
                    format!("claude mcp add --scope user plumivox -- {exe_text} --stdio"),
                    "codex mcp get plumivox".to_string(),
                    format!("codex mcp add plumivox -- {exe_text} --stdio"),
                ];
                ensure!(
                    lines.iter().map(|l| normal(l)).collect::<Vec<_>>()
                        == expected.iter().map(|l| normal(l)).collect::<Vec<_>>(),
                    "unexpected CLI calls: {lines:?}"
                );
                for tool in ["claude", "codex"] {
                    ensure!(
                        World::points_at(&world.registry(tool), &exe),
                        "{tool} not registered"
                    );
                }
                let run = run_value(&format!("{PACK_ID} Update"))?.context("Run value missing")?;
                evidence.insert("cycle1-run-value".into(), json!(run));
                ensure!(
                    normal(&run).ends_with(&normal(&format!(
                        "\" --headless \"{exe_text}\" --update-apply"
                    ))) && normal(&run).contains("system32/conhost.exe"),
                    "unexpected Run value: {run}"
                );
                let state = world.registration();
                evidence.insert("cycle1-state".into(), state.clone());
                ensure!(
                    state["done"] == json!(["claude", "codex"])
                        && state["logon"] == true
                        && state["version"] == "9.0.0"
                        && normal(state["executable"].as_str().unwrap_or("")) == normal(&exe_text),
                    "unexpected registration state: {state}"
                );
                Ok(())
            }
        );

        if first_installed {
            case!(
                "cycle 1 uninstall removes own registrations and Run value",
                {
                    world.uninstall(&first)?;
                    for tool in ["claude", "codex"] {
                        ensure!(
                            world.registry(tool).get("plumivox").is_none(),
                            "{tool} registration remained after uninstall"
                        );
                    }
                    ensure!(
                        run_value(&format!("{PACK_ID} Update"))?.is_none(),
                        "Run value remained after uninstall"
                    );
                    Ok(())
                }
            );
        }

        // ───── Toolport が既に提供している環境: 直接登録を省き、ゲートウェイが消えたら直接登録へ戻す ─────
        let toolport_install = world.install_dir("toolport");
        case!(
            "(13) a Toolport-served install skips direct registration and falls back when the gateway disappears",
            {
                let env = world.root.join("env");
                let fixture = world.root.join("toolport-fixture");
                fs::create_dir_all(&fixture)?;
                let gateway = fixture.join("toolport-gateway.exe");
                // Setup の固定パスとは別の場所のコピー（ファイル名と --stdio で見る）。
                let product = fixture.join("plumivox.exe");
                fs::copy(&stub, &gateway)?;
                fs::copy(&stub, &product)?;
                let claude_json = env.join("claude-config").join(".claude.json");
                let codex_toml = env.join("codex-home").join("config.toml");
                let registry = env.join("appdata").join("Toolport").join("registry.json");
                fs::create_dir_all(registry.parent().context("registry parent")?)?;
                fs::write(
                    &claude_json,
                    serde_json::to_vec(&json!({"mcpServers": {"toolport": {
                        "type": "stdio", "command": gateway.to_string_lossy(), "args": []
                    }}}))?,
                )?;
                fs::write(
                    &codex_toml,
                    format!(
                        "[mcp_servers.toolport]\ncommand = '{}'\nargs = []\n",
                        gateway.display()
                    ),
                )?;
                fs::write(
                    &registry,
                    serde_json::to_vec(&json!({
                        "version": 3,
                        "servers": [{
                            "id": "plumivox", "transport": "stdio", "enabled": true,
                            "command": product.to_string_lossy(), "args": ["--stdio"]
                        }],
                        "profiles": [{"id": "p1", "name": "main", "enabledServerIds": ["plumivox"]}],
                        "activeProfileId": "p1"
                    }))?,
                )?;
                let calls_before = world.calls()?.len();
                world.install(&setup, &toolport_install)?;
                let lines = world.call_lines(calls_before)?;
                evidence.insert("toolport-install-calls".into(), json!(lines));
                ensure!(
                    lines.is_empty(),
                    "a served install must not call the CLIs: {lines:?}"
                );
                let state = world.registration();
                evidence.insert("toolport-state".into(), state.clone());
                ensure!(
                    state["done"] == json!([])
                        && state["skipped"] == json!({"claude": "toolport", "codex": "toolport"})
                        && state["logon"] == true,
                    "unexpected state after a served install: {state}"
                );
                // Claude Code のゲートウェイが消えたら、更新の適用（ensure）で直接登録に戻す。Codex は提供中のまま。
                fs::remove_file(&claude_json)?;
                let (code, _) = world.update_apply(&toolport_install, false)?;
                ensure!(code == 0, "--update-apply exited with {code}");
                let exe = World::exe(&toolport_install);
                ensure!(
                    World::points_at(&world.registry("claude"), &exe),
                    "claude was not registered after the gateway disappeared"
                );
                ensure!(
                    world.registry("codex").get("plumivox").is_none(),
                    "codex must stay unregistered while Toolport serves it"
                );
                let state = world.registration();
                evidence.insert("toolport-fallback-state".into(), state.clone());
                ensure!(
                    state["done"] == json!(["claude"])
                        && state["skipped"] == json!({"codex": "toolport"}),
                    "unexpected state after the fallback: {state}"
                );
                world.uninstall(&toolport_install)?;
                // 以降の導入は Toolport なしの環境で行う。
                fs::remove_file(&codex_toml)?;
                fs::remove_file(&registry)?;
                world.set_registry("claude", None)?;
                Ok(())
            }
        );

        // ───── 2回目の導入: 旧 dist の置き換え・自分の登録の保持・フックの3秒打ち切り ─────
        let calls_before_second = world.calls()?.len();
        world.set_registry("claude", Some(&world.legacy))?;
        world.set_registry("codex", Some(&World::exe(&second)))?;
        world.set_faults(json!({"claude": {"add_delay_ms": 5000}}))?;
        let second_installed = case!(
            "(2)(3) hook replaces the legacy dist entry, keeps its own, and a slow add is left unfinished",
            {
                world.install(&setup, &second)?;
                let lines = world.call_lines(calls_before_second)?;
                evidence.insert("cycle2-install-calls".into(), json!(lines));
                let exe_text = World::exe(&second).to_string_lossy().into_owned();
                let expected = [
                    "claude mcp get plumivox".to_string(),
                    "claude mcp remove plumivox --scope user".to_string(),
                    format!("claude mcp add --scope user plumivox -- {exe_text} --stdio"),
                    "codex mcp get plumivox".to_string(),
                ];
                ensure!(
                    lines.iter().map(|l| normal(l)).collect::<Vec<_>>()
                        == expected.iter().map(|l| normal(l)).collect::<Vec<_>>(),
                    "unexpected hook CLI calls: {lines:?}"
                );
                let state = world.registration();
                evidence.insert("cycle2-hook-state".into(), state.clone());
                ensure!(
                    state["done"] == json!(["codex"]) && state["logon"] == true,
                    "the timed-out claude registration must stay unfinished: {state}"
                );
                ensure!(
                    world.registry("claude").get("plumivox").is_none(),
                    "the killed add must not leave a registration"
                );
                ensure!(
                    World::points_at(&world.registry("codex"), &World::exe(&second)),
                    "own codex registration changed"
                );
                Ok(())
            }
        );
        if second_installed {
            case!("(3) --update-apply finishes the unfinished registration", {
                world.set_faults(json!({}))?;
                let from = world.calls()?.len();
                let (code, value) = world.update_apply(&second, true)?;
                ensure!(
                    code == 0 && value["status"] == "nothingPending",
                    "unexpected --update-apply: {code} {value}"
                );
                let lines = world.call_lines(from)?;
                evidence.insert("cycle2-retry-calls".into(), json!(lines));
                let exe_text = World::exe(&second).to_string_lossy().into_owned();
                ensure!(
                    lines.iter().map(|l| normal(l)).collect::<Vec<_>>()
                        == [
                            normal("claude mcp get plumivox"),
                            normal(&format!(
                                "claude mcp add --scope user plumivox -- {exe_text} --stdio"
                            ))
                        ],
                    "unexpected retry calls: {lines:?}"
                );
                ensure!(
                    world.registration()["done"] == json!(["claude", "codex"]),
                    "retry did not complete the registration"
                );
                ensure!(
                    World::points_at(&world.registry("claude"), &World::exe(&second)),
                    "claude does not point at the installed executable"
                );
                Ok(())
            });

            case!(
                "(4)(11) unchanged restarts call no CLI; installed MCP initializes with JSON-RPC-only stdout",
                {
                    let from = world.calls()?.len();
                    let (code, value) = world.update_apply(&second, true)?;
                    ensure!(code == 0 && value["status"] == "nothingPending", "{value}");
                    let log_before = world.log().len();
                    // McpClient は標準出力の非 JSON-RPC の行を失敗として扱う（initialize の成功も確かめる）。
                    let mut client = world.mcp(&second).await?;
                    let tools = client.request("tools/list", json!({})).await?;
                    ensure!(
                        tools["tools"]
                            .as_array()
                            .is_some_and(|items| !items.is_empty()),
                        "tools/list returned no tools"
                    );
                    // 背景スレッドは登録の確認の後に更新を確認する。その記録が出るまで待ってから数える。
                    wait_for(Duration::from_secs(60), "the background check", || {
                        Ok(world.log()[log_before..].contains("auto-update: up to date"))
                    })
                    .await?;
                    client.close().await?;
                    let lines = world.call_lines(from)?;
                    ensure!(
                        lines.is_empty(),
                        "unchanged restarts called CLIs: {lines:?}"
                    );
                    Ok(())
                }
            );

            case!(
                "(9) three concurrent MCPs download the update exactly once",
                {
                    copy_feed(&feed_v2, &world.feed)?;
                    // 前の確認の時刻を消し、確認の時期にする（他の項目は無い）。
                    fs::write(
                        world.state_dir().join("app-update.json"),
                        b"{\"auto_update\":true}\n",
                    )?;
                    let log_before = world.log().len();
                    let (a, b, c) =
                        tokio::join!(world.mcp(&second), world.mcp(&second), world.mcp(&second));
                    harness.clients.extend([a?, b?, c?]);
                    wait_for(Duration::from_secs(90), "the download", || {
                        Ok(world.log()[log_before..].contains("auto-update: downloaded 9.0.1"))
                    })
                    .await?;
                    // 他の2つが遅れて確認しないことを確かめるため、少し待ってから数える。
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    let log = world.log()[log_before..].to_string();
                    evidence.insert("cycle2-download-log".into(), json!(log));
                    ensure!(
                        log.matches("auto-update").count() == 1,
                        "the update was checked more than once: {log}"
                    );
                    ensure!(
                        second
                            .join("packages")
                            .join(format!("{PACK_ID}-9.0.1-full.nupkg"))
                            .is_file(),
                        "the downloaded package is missing"
                    );
                    Ok(())
                }
            );

            case!("(5) running MCPs keep the update pending (busy)", {
                let (code, value) = world.update_apply(&second, true)?;
                ensure!(
                    code == 0
                        && value["status"] == "busy"
                        && value["processes"].as_u64() >= Some(3),
                    "expected busy: {value}"
                );
                ensure!(
                    world.version(&second)? == "plumivox 9.0.0",
                    "version changed while busy"
                );
                // 登録された Run 値のコマンドそのもの（conhost --headless 経由）でも同じ判定になることを、
                // 診断ログの行で確かめる（ログオン時に実際に起動される形）。
                let registered =
                    run_value(&format!("{PACK_ID} Update"))?.context("Run value missing")?;
                let system = std::env::var("SystemRoot").context("SystemRoot")?;
                let conhost = format!("{system}\\System32\\conhost.exe");
                let exe = World::exe(&second);
                ensure!(
                    normal(&registered)
                        == normal(&format!(
                            "\"{conhost}\" --headless \"{}\" --update-apply",
                            exe.display()
                        )),
                    "unexpected Run value: {registered}"
                );
                let log_before = world.log().len();
                // conhost --headless は標準ハンドルを渡されると（パイプでも NUL でも）子を道連れにして終わる。
                // std の Command は常に STARTF_USESTDHANDLES で標準ハンドルを渡すため、ログオン時（Explorer の
                // Run）と同じく標準ハンドルを渡さない CreateProcessW で起動する。
                let mut environment = world.environment();
                environment.push(("PLUMIVOX_UPDATE_ALLOW_UNSIGNED".into(), "1".into()));
                let command_line = format!(
                    "\"{conhost}\" --headless \"{}\" --update-apply",
                    exe.display()
                );
                let code = tokio::task::spawn_blocking(move || {
                    spawn_without_std_handles(&command_line, &environment, Duration::from_secs(120))
                })
                .await??;
                evidence.insert("cycle2-run-command-exit".into(), json!(code));
                ensure!(code == 0, "the Run command failed ({code})");
                // conhost が子の終了より先に戻る場合に備え、記録が出るまで少し待つ。
                let waited = wait_for(Duration::from_secs(30), "the Run command log", || {
                    Ok(world.log()[log_before..]
                        .contains("update-apply: skipped, 3 other process(es)"))
                })
                .await;
                let log = world.log()[log_before..].to_string();
                evidence.insert("cycle2-run-command-log".into(), json!(log));
                ensure!(waited.is_ok(), "the Run command did not report busy: {log}");
                ensure!(
                    world.version(&second)? == "plumivox 9.0.0",
                    "version changed while busy"
                );
                Ok(())
            });

            case!(
                "(6) a lingering shared daemon alone keeps the update pending",
                {
                    let client = harness.clients.first().context("no MCP client")?;
                    let raw = tokio::time::timeout(
                        Duration::from_secs(30),
                        client.call_tool("start_proxy", json!({"account":"managed"})),
                    )
                    .await
                    .context("start_proxy timed out")??;
                    let data = harness.business(&raw)?;
                    ensure!(
                        raw["isError"] != true && super::proxy_support::state(&data) == "ready",
                        "start_proxy failed: {}",
                        data["error"]["code"]
                    );
                    for client in harness.clients.iter_mut() {
                        client.close().await?;
                    }
                    harness.clients.clear();
                    let remaining = processes_under(&second);
                    evidence.insert("cycle2-daemon-processes".into(), json!(remaining));
                    ensure!(
                        remaining.len() == 1,
                        "expected only the daemon: {remaining:?}"
                    );
                    let (code, value) = world.update_apply(&second, true)?;
                    ensure!(
                        code == 0 && value["status"] == "busy" && value["processes"] == 1,
                        "expected busy for the daemon: {value}"
                    );
                    wait_for(Duration::from_secs(60), "the daemon to exit", || {
                        Ok(processes_under(&second).is_empty())
                    })
                    .await?;
                    Ok(())
                }
            );

            case!(
                "(7) an unsigned package is refused without the test override",
                {
                    let (code, value) = world.update_apply(&second, false)?;
                    ensure!(
                        code == 2 && value["status"] == "untrusted",
                        "expected untrusted: {code} {value}"
                    );
                    ensure!(
                        world.version(&second)? == "plumivox 9.0.0",
                        "untrusted update applied"
                    );
                    Ok(())
                }
            );

            let calls_before_apply = world.calls()?.len();
            case!(
                "(6) after the daemon exits the update is applied and --version is 9.0.1",
                {
                    let (code, value) = world.update_apply(&second, true)?;
                    ensure!(
                        code == 0 && value == json!({"status":"applied","version":"9.0.1"}),
                        "expected applied: {code} {value}"
                    );
                    wait_for(Duration::from_secs(120), "the new version", || {
                        Ok(world.version(&second).is_ok_and(|v| v == "plumivox 9.0.1"))
                    })
                    .await?;
                    let manifest = fs::read_to_string(second.join("current").join("sq.version"))?;
                    ensure!(
                        manifest.contains("<version>9.0.1</version>"),
                        "sq.version: {manifest}"
                    );
                    Ok(())
                }
            );

            case!(
                "(8) the update hook refreshes only the logon entry and records 9.0.1",
                {
                    wait_for(Duration::from_secs(60), "the updated hook", || {
                        Ok(world.registration()["version"] == "9.0.1")
                    })
                    .await?;
                    let lines = world.call_lines(calls_before_apply)?;
                    ensure!(lines.is_empty(), "the update called CLIs: {lines:?}");
                    let state = world.registration();
                    ensure!(
                        state["done"] == json!(["claude", "codex"]) && state["logon"] == true,
                        "{state}"
                    );
                    let run =
                        run_value(&format!("{PACK_ID} Update"))?.context("Run value missing")?;
                    ensure!(
                        normal(&run).contains(&normal(&World::exe(&second).to_string_lossy())),
                        "Run value no longer points at the install: {run}"
                    );
                    Ok(())
                }
            );

            case!(
                "(10) uninstall removes only its own entries and keeps the data folder",
                {
                    // 別の場所を指すようにした codex の登録は残るはず。
                    world.set_registry("codex", Some(&world.legacy))?;
                    let sentinel = world.data.join("velopack-e2e-sentinel.txt");
                    fs::write(&sentinel, b"keep me")?;
                    let settings_before = fs::read(world.state_dir().join("app-update.json"))?;
                    let from = world.calls()?.len();
                    world.uninstall(&second)?;
                    let lines = world.call_lines(from)?;
                    evidence.insert("cycle2-uninstall-calls".into(), json!(lines));
                    ensure!(
                        world.registry("claude").get("plumivox").is_none(),
                        "own claude registration remained"
                    );
                    ensure!(
                        World::points_at(&world.registry("codex"), &world.legacy),
                        "a foreign codex registration was removed"
                    );
                    ensure!(
                        !lines
                            .iter()
                            .any(|line| normal(line).starts_with("codex mcp remove")),
                        "a foreign registration was removed: {lines:?}"
                    );
                    ensure!(
                        run_value(&format!("{PACK_ID} Update"))?.is_none(),
                        "Run value remained"
                    );
                    ensure!(
                        fs::read(&sentinel)? == b"keep me"
                            && fs::read(world.state_dir().join("app-update.json"))?
                                == settings_before,
                        "the data folder changed"
                    );
                    Ok(())
                }
            );
        }

        // ───── 後片付けと本物の環境の不変 ─────
        let leftovers = uninstall_leftovers(
            &world,
            &[first.clone(), toolport_install.clone(), second.clone()],
        );
        evidence.insert("leftover-uninstalls".into(), json!(leftovers));
        let _ = harness.close().await;
        case!(
            "(12) no test Run/Uninstall entries remain and production entries are unchanged",
            {
                ensure!(
                    run_value(&format!("{PACK_ID} Update"))?.is_none(),
                    "the test Run value remains"
                );
                let uninstall = reg_query(&format!(r"{UNINSTALL_KEY}\{PACK_ID}"), None)?;
                ensure!(
                    uninstall.is_none(),
                    "the test Uninstall entry remains: {uninstall:?}"
                );
                for install in [&first, &second] {
                    let remaining: Vec<String> = fs::read_dir(install)
                        .map(|entries| {
                            entries
                                .filter_map(|entry| entry.ok())
                                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                                .collect()
                        })
                        .unwrap_or_default();
                    evidence.insert(
                        format!("install-left:{}", install.display()),
                        json!(remaining),
                    );
                }
                let after = production_snapshot()?;
                evidence.insert("production-before".into(), before.clone());
                evidence.insert("production-after".into(), after.clone());
                ensure!(
                    before == after,
                    "production entries changed: {before} -> {after}"
                );
                Ok(())
            }
        );
        evidence.insert("stub-calls".into(), json!(world.call_lines(0)?));
        evidence.insert("app-update-log".into(), json!(world.log()));
        suite.write_json("velopack-evidence.json", &json!(evidence))?;
        suite.finish()
    }
}
