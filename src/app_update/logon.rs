//! ログオン時に更新の適用（`plumivox --update-apply`）を起動する登録。
//!
//! 多プロセス型の Plumivox は、日中に適用すると他のセッションの MCP や共有中継 daemon を強制終了してしまう
//! ため（Velopack の適用は導入先の自分以外の全プロセスを止める）、適用はログオン時だけに行う。
//! その起動点をここで登録・解除する。
//!
//! - Windows: HKCU の Run キーに値を1つ置く。値名は packId に紐づけ（`<packId> Update`）、
//!   別の導入先を指す同名の値は上書きも削除もしない（同じ packId の別の導入や、他の製品の値を壊さない）。
//! - macOS: `~/Library/LaunchAgents/com.kagayoi.<packId>.update.plist` を置く（`RunAtLoad` で
//!   ログイン時に1回だけ `--update-apply` を起動する）。Windows と同じく、別の導入先を指す同名の plist は
//!   上書きも削除もしない。登録時に `launchctl bootstrap` はしない（次のログインで読み込ませるため）。
//!   解除時は、読み込み済みなら `launchctl bootout` してから plist を消す。
//! - その他の OS（Linux）は Setup 配布の対象外なので、登録しない（エラーを返す）。
use std::path::Path;

/// 登録の結果。
#[derive(Debug, PartialEq, Eq)]
pub enum Registration {
    /// 値を新しく作った。
    Created,
    /// 自分の導入先を指す既存の値を、最新の定義へ更新した。
    Updated,
    /// 既に同じ定義だった。
    Unchanged,
    /// 別の導入先を指す同名の値があるため、触らなかった。
    ForeignKept,
}

/// Run キーの値名。packId に紐づけ、他の製品や別の packId の値と衝突しない。
#[cfg_attr(not(windows), allow(dead_code))]
pub fn value_name(app_id: &str) -> String {
    format!("{app_id} Update")
}

/// Run キーに書くコマンド。
///
/// plumivox.exe はコンソール版のため、そのまま起動するとログオンのたびにコンソールが一瞬出る。
/// `conhost.exe --headless` 経由にして、ウィンドウを作らずに起動する。
/// パスは二重引用符で囲む（空白を含む導入先でも1つの引数になる）。
#[cfg_attr(not(windows), allow(dead_code))]
pub fn run_command(executable: &Path) -> String {
    let conhost = std::env::var_os("SystemRoot")
        .map(|root| Path::new(&root).join("System32").join("conhost.exe"))
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| "conhost.exe".to_string());
    format!(
        "\"{conhost}\" --headless \"{}\" --update-apply",
        executable.display()
    )
}

/// 既存の値が、この導入先の実行ファイルを指しているか（引用符で囲んだパスを大小無視で含むか）。
#[cfg_attr(not(windows), allow(dead_code))]
pub fn points_at(command: &str, executable: &Path) -> bool {
    let needle = format!("\"{}\"", executable.display()).to_ascii_lowercase();
    command.to_ascii_lowercase().contains(&needle)
}

/// 登録する。値が無ければ作り、自分を指す値なら更新し、別の場所を指す値は触らない。
pub fn register(app_id: &str, executable: &Path) -> std::io::Result<Registration> {
    #[cfg(windows)]
    {
        windows::register_in(
            &windows::RegKey::current_user(),
            windows::RUN_SUBKEY,
            app_id,
            executable,
        )
    }
    #[cfg(target_os = "macos")]
    {
        macos::register_in(&macos::launch_agents_dir(), app_id, executable)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (app_id, executable);
        Err(std::io::Error::other(
            "logon apply is not supported on this OS",
        ))
    }
}

/// 登録先の場所（診断・案内の表示用）。Windows は Run キーの値名、macOS は plist のパス。
pub fn describe(app_id: &str) -> String {
    #[cfg(windows)]
    {
        format!(r"HKCU\{}\{}", windows::RUN_SUBKEY, value_name(app_id))
    }
    #[cfg(target_os = "macos")]
    {
        macos::plist_path(&macos::launch_agents_dir(), app_id)
            .display()
            .to_string()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = app_id;
        "unsupported".to_string()
    }
}

/// 解除する。自分の導入先を指す値だけを消し、消したかを返す。
pub fn unregister(app_id: &str, executable: &Path) -> std::io::Result<bool> {
    #[cfg(windows)]
    {
        windows::unregister_in(
            &windows::RegKey::current_user(),
            windows::RUN_SUBKEY,
            app_id,
            executable,
        )
    }
    #[cfg(target_os = "macos")]
    {
        let dir = macos::launch_agents_dir();
        if macos::points_at_file(&dir, app_id, executable) {
            // 読み込み済みのジョブを外す（未読み込みなら失敗するが、それで構わない）。plist を消すだけでは、
            // ログアウトまで launchd に定義が残るため。
            macos::bootout(app_id);
        }
        macos::unregister_in(&dir, app_id, executable)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (app_id, executable);
        Ok(false)
    }
}

/// macOS の LaunchAgent（ログイン時に `--update-apply` を1回起動する）。Remnifold の実機検証済みの定義に揃える。
///
/// 設計の要点:
/// - ラベルは `com.kagayoi.<packId>.update`。packId に紐づけ、テスト用 packId と本番が衝突しない。
/// - `RunAtLoad` だけを立て、`KeepAlive` は付けない（ログインごとに1回走って終わる）。
/// - `AbandonProcessGroup` を立てる。launchd は既定で、ジョブの主プロセスが終わると同じプロセスグループに
///   残った子を終了させる。`--update-apply` は自分の終了を待って入れ替える UpdateMac を子として起動するため、
///   これが無いと入れ替えの途中で UpdateMac が止められてしまう。
#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use std::{io, path::PathBuf};

    /// 本番の置き場。`HOME` を差し替えた隔離検証では、その配下になる。
    pub fn launch_agents_dir() -> PathBuf {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .or_else(dirs::home_dir)
            .unwrap_or_default()
            .join("Library")
            .join("LaunchAgents")
    }

    /// launchd のラベル（逆 DNS 形式）。
    pub fn label(app_id: &str) -> String {
        format!("com.kagayoi.{app_id}.update")
    }

    pub fn plist_path(dir: &Path, app_id: &str) -> PathBuf {
        dir.join(format!("{}.plist", label(app_id)))
    }

    /// plist の文字列要素に入れるための XML エスケープ。
    fn escape(text: &str) -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }

    /// 書き込む plist の本文。
    pub fn plist_body(app_id: &str, executable: &Path) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n\t<key>Label</key>\n\t<string>{label}</string>\n\t<key>ProgramArguments</key>\n\t<array>\n\t\t<string>{exe}</string>\n\t\t<string>--update-apply</string>\n\t</array>\n\t<key>RunAtLoad</key>\n\t<true/>\n\t<key>AbandonProcessGroup</key>\n\t<true/>\n\t<key>ProcessType</key>\n\t<string>Background</string>\n</dict>\n</plist>\n",
            label = escape(&label(app_id)),
            exe = escape(&executable.to_string_lossy()),
        )
    }

    /// plist の本文が、この実行ファイルを起動するものか（エスケープ済みのパスの文字列要素を含むか）。
    pub fn points_at(body: &str, executable: &Path) -> bool {
        body.contains(&format!(
            "<string>{}</string>",
            escape(&executable.to_string_lossy())
        ))
    }

    /// 既存の plist（通常ファイル）を読む。無ければ None。リンクなど通常ファイルでないものは、
    /// 他の導入のものとして扱えるよう空文字を返す（上書きも削除もしない側へ倒す）。
    fn read_existing(path: &Path) -> io::Result<Option<String>> {
        match std::fs::symlink_metadata(path) {
            Ok(meta) if meta.is_file() => Ok(Some(
                String::from_utf8_lossy(&std::fs::read(path)?).into_owned(),
            )),
            Ok(_) => Ok(Some(String::new())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn points_at_file(dir: &Path, app_id: &str, executable: &Path) -> bool {
        read_existing(&plist_path(dir, app_id))
            .ok()
            .flatten()
            .is_some_and(|body| points_at(&body, executable))
    }

    fn write(path: &Path, body: &str) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::app_update::atomic_write(path, body.as_bytes()).map_err(io::Error::other)
    }

    pub fn register_in(dir: &Path, app_id: &str, executable: &Path) -> io::Result<Registration> {
        let path = plist_path(dir, app_id);
        let desired = plist_body(app_id, executable);
        match read_existing(&path)? {
            None => {
                write(&path, &desired)?;
                Ok(Registration::Created)
            }
            Some(existing) if existing == desired => Ok(Registration::Unchanged),
            // 自分の導入先を指す plist だけを最新の定義へ書き直す。
            Some(existing) if points_at(&existing, executable) => {
                write(&path, &desired)?;
                Ok(Registration::Updated)
            }
            // 別の場所を指す同名の plist は、他の導入のものとして触らない。
            Some(_) => Ok(Registration::ForeignKept),
        }
    }

    pub fn unregister_in(dir: &Path, app_id: &str, executable: &Path) -> io::Result<bool> {
        let path = plist_path(dir, app_id);
        match read_existing(&path)? {
            Some(existing) if points_at(&existing, executable) => {
                std::fs::remove_file(&path)?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// ログイン中の利用者の GUI ドメインから、このジョブを外す。失敗（未読み込みなど）は無視する。
    pub fn bootout(app_id: &str) {
        // SAFETY: getuid は引数を取らず、失敗しない。
        let uid = unsafe { libc::getuid() };
        let _ = std::process::Command::new("/bin/launchctl")
            .arg("bootout")
            .arg(format!("gui/{uid}/{}", label(app_id)))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn agent_is_created_updated_and_removed_only_for_the_own_install() {
            let dir = crate::app_update::tests::scratch("agents");
            let mine = Path::new("/Users/a b/Apps/Plumivox.app/Contents/MacOS/plumivox");
            let other = Path::new("/Applications/Plumivox.app/Contents/MacOS/plumivox");
            std::fs::write(dir.join("com.example.other.plist"), "other").unwrap();
            assert_eq!(
                register_in(&dir, "TestApp", mine).unwrap(),
                Registration::Created
            );
            assert_eq!(
                register_in(&dir, "TestApp", mine).unwrap(),
                Registration::Unchanged
            );
            let path = plist_path(&dir, "TestApp");
            let body = std::fs::read_to_string(&path).unwrap();
            assert!(points_at(&body, mine) && !points_at(&body, other));
            for needed in [
                "<string>--update-apply</string>",
                "<key>RunAtLoad</key>",
                "<key>AbandonProcessGroup</key>",
                "<string>com.kagayoi.TestApp.update</string>",
            ] {
                assert!(body.contains(needed), "{needed}");
            }
            assert!(!body.contains("KeepAlive"));
            assert_eq!(
                register_in(&dir, "TestApp", other).unwrap(),
                Registration::ForeignKept
            );
            assert!(!unregister_in(&dir, "TestApp", other).unwrap());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), body);
            std::fs::write(&path, format!("<string>{}</string>", mine.display())).unwrap();
            assert_eq!(
                register_in(&dir, "TestApp", mine).unwrap(),
                Registration::Updated
            );
            assert!(unregister_in(&dir, "TestApp", mine).unwrap());
            assert!(!path.exists());
            assert_eq!(
                std::fs::read_to_string(dir.join("com.example.other.plist")).unwrap(),
                "other"
            );
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

#[cfg(windows)]
pub(crate) mod windows {
    use super::*;
    use std::{io, ptr::null_mut};
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_EXPAND_SZ,
        REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteValueW,
        RegQueryValueExW, RegSetValueExW,
    };

    /// HKCU の Run キー。
    pub const RUN_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const ERROR_SUCCESS: u32 = 0;
    const ERROR_FILE_NOT_FOUND: u32 = 2;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn check(status: u32) -> io::Result<()> {
        if status == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(status as i32))
        }
    }

    /// 根にするキー。本番は HKCU、テストはファイルに閉じた専用のハイブ（実際の Run キーに触れない）。
    pub struct RegKey {
        handle: HKEY,
        owned: bool,
    }

    impl RegKey {
        pub fn current_user() -> Self {
            Self {
                handle: HKEY_CURRENT_USER,
                owned: false,
            }
        }

        /// ファイルに閉じた専用のハイブを開く（RegLoadAppKeyW。このプロセスだけから見える）。
        #[cfg(test)]
        pub fn load_app_hive(file: &Path) -> io::Result<Self> {
            use windows_sys::Win32::System::Registry::{
                KEY_ALL_ACCESS, REG_PROCESS_APPKEY, RegLoadAppKeyW,
            };
            let path = wide(&file.to_string_lossy());
            let mut handle: HKEY = null_mut();
            // SAFETY: path は NUL 終端、handle は書き込み先の変数。
            check(unsafe {
                RegLoadAppKeyW(
                    path.as_ptr(),
                    &mut handle,
                    KEY_ALL_ACCESS,
                    REG_PROCESS_APPKEY,
                    0,
                )
            })?;
            Ok(Self {
                handle,
                owned: true,
            })
        }

        fn create_subkey(&self, subkey: &str) -> io::Result<Self> {
            let name = wide(subkey);
            let mut handle: HKEY = null_mut();
            // SAFETY: name は NUL 終端、handle は書き込み先の変数。
            check(unsafe {
                RegCreateKeyExW(
                    self.handle,
                    name.as_ptr(),
                    0,
                    std::ptr::null(),
                    REG_OPTION_NON_VOLATILE,
                    KEY_QUERY_VALUE | KEY_SET_VALUE,
                    std::ptr::null(),
                    &mut handle,
                    null_mut(),
                )
            })?;
            Ok(Self {
                handle,
                owned: true,
            })
        }

        /// 文字列の値を読む（無ければ None。文字列型以外の値は空文字として扱い、上書きしない側へ倒す）。
        fn query_string(&self, name: &str) -> io::Result<Option<String>> {
            let name = wide(name);
            let mut kind = 0u32;
            let mut size = 0u32;
            // SAFETY: 大きさだけを問い合わせる（データ領域は null）。
            let status = unsafe {
                RegQueryValueExW(
                    self.handle,
                    name.as_ptr(),
                    std::ptr::null(),
                    &mut kind,
                    null_mut(),
                    &mut size,
                )
            };
            if status == ERROR_FILE_NOT_FOUND {
                return Ok(None);
            }
            check(status)?;
            let mut buffer = vec![0u8; size as usize + 2];
            let mut length = size;
            // SAFETY: buffer は length バイト以上の書き込み可能な領域。
            check(unsafe {
                RegQueryValueExW(
                    self.handle,
                    name.as_ptr(),
                    std::ptr::null(),
                    &mut kind,
                    buffer.as_mut_ptr(),
                    &mut length,
                )
            })?;
            if kind != REG_SZ && kind != REG_EXPAND_SZ {
                return Ok(Some(String::new()));
            }
            let units: Vec<u16> = buffer[..length as usize]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .take_while(|unit| *unit != 0)
                .collect();
            Ok(Some(String::from_utf16_lossy(&units)))
        }

        fn set_string(&self, name: &str, value: &str) -> io::Result<()> {
            let name = wide(name);
            let data: Vec<u8> = wide(value)
                .iter()
                .flat_map(|unit| unit.to_le_bytes())
                .collect();
            // SAFETY: data は NUL 終端を含む UTF-16LE のバイト列。
            check(unsafe {
                RegSetValueExW(
                    self.handle,
                    name.as_ptr(),
                    0,
                    REG_SZ,
                    data.as_ptr(),
                    data.len() as u32,
                )
            })
        }

        fn delete_value(&self, name: &str) -> io::Result<()> {
            let name = wide(name);
            // SAFETY: name は NUL 終端。
            let status = unsafe { RegDeleteValueW(self.handle, name.as_ptr()) };
            if status == ERROR_FILE_NOT_FOUND {
                return Ok(());
            }
            check(status)
        }
    }

    impl Drop for RegKey {
        fn drop(&mut self) {
            if self.owned {
                // SAFETY: owned は自分が開いたハンドルだけに立つ。
                unsafe { RegCloseKey(self.handle) };
            }
        }
    }

    pub fn register_in(
        root: &RegKey,
        subkey: &str,
        app_id: &str,
        executable: &Path,
    ) -> io::Result<Registration> {
        let key = root.create_subkey(subkey)?;
        let name = value_name(app_id);
        let desired = run_command(executable);
        match key.query_string(&name)? {
            None => {
                key.set_string(&name, &desired)?;
                Ok(Registration::Created)
            }
            Some(existing) if existing == desired => Ok(Registration::Unchanged),
            // 自分の導入先を指す値だけを最新の定義へ張り直す。
            Some(existing) if points_at(&existing, executable) => {
                key.set_string(&name, &desired)?;
                Ok(Registration::Updated)
            }
            // 別の場所を指す同名の値は、他の導入のものとして触らない。
            Some(_) => Ok(Registration::ForeignKept),
        }
    }

    pub fn unregister_in(
        root: &RegKey,
        subkey: &str,
        app_id: &str,
        executable: &Path,
    ) -> io::Result<bool> {
        let key = root.create_subkey(subkey)?;
        let name = value_name(app_id);
        match key.query_string(&name)? {
            Some(existing) if points_at(&existing, executable) => {
                key.delete_value(&name)?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn scratch_hive() -> (RegKey, std::path::PathBuf) {
            let dir = crate::app_update::tests::scratch("run-hive");
            (RegKey::load_app_hive(&dir.join("hive.dat")).unwrap(), dir)
        }

        #[test]
        fn run_value_is_created_updated_and_removed_only_for_the_own_install() {
            let (hive, dir) = scratch_hive();
            let mine = Path::new(r"C:\Apps\PlumivoxApp\current\plumivox.exe");
            let other = Path::new(r"D:\Elsewhere\current\plumivox.exe");
            // 他の製品の値は、登録・解除のどちらでも変わらない。
            let key = hive.create_subkey(RUN_SUBKEY).unwrap();
            key.set_string("Other", "other.exe").unwrap();

            assert_eq!(
                register_in(&hive, RUN_SUBKEY, "TestApp", mine).unwrap(),
                Registration::Created
            );
            assert_eq!(
                register_in(&hive, RUN_SUBKEY, "TestApp", mine).unwrap(),
                Registration::Unchanged
            );
            // 別の導入先からの登録は、既存の値を上書きしない。
            assert_eq!(
                register_in(&hive, RUN_SUBKEY, "TestApp", other).unwrap(),
                Registration::ForeignKept
            );
            assert!(!unregister_in(&hive, RUN_SUBKEY, "TestApp", other).unwrap());
            let value = key.query_string("TestApp Update").unwrap().unwrap();
            assert!(points_at(&value, mine) && !points_at(&value, other));
            assert!(value.contains("conhost.exe\" --headless \""));
            assert!(value.ends_with("plumivox.exe\" --update-apply"));
            // 古い定義（コマンドの形が違う）でも、自分を指していれば張り直す。
            key.set_string(
                "TestApp Update",
                &format!("\"{}\" --update-apply", mine.display()),
            )
            .unwrap();
            assert_eq!(
                register_in(&hive, RUN_SUBKEY, "TestApp", mine).unwrap(),
                Registration::Updated
            );
            assert!(unregister_in(&hive, RUN_SUBKEY, "TestApp", mine).unwrap());
            assert!(key.query_string("TestApp Update").unwrap().is_none());
            assert_eq!(
                key.query_string("Other").unwrap().as_deref(),
                Some("other.exe")
            );
            // 無い値の解除は何もしない。
            assert!(!unregister_in(&hive, RUN_SUBKEY, "TestApp", mine).unwrap());
            drop(key);
            drop(hive);
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_quotes_the_path_and_names_the_option() {
        let exe = Path::new(r"C:\Users\a b\AppData\Local\PlumivoxApp\current\plumivox.exe");
        let command = run_command(exe);
        assert!(command.contains("conhost.exe\" --headless "));
        assert!(command.ends_with(&format!("\"{}\" --update-apply", exe.display())));
        assert!(points_at(&command, exe));
        assert!(!points_at(&command, Path::new(r"C:\Other\plumivox.exe")));
        assert_eq!(value_name("PlumivoxApp"), "PlumivoxApp Update");
    }
}
