mod ci;
mod r2;
#[cfg(windows)]
mod windows_signing;

use serde_json::Value;
use std::{
    collections::BTreeMap,
    env,
    error::Error,
    ffi::OsStr,
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .into()
}
fn cargo() -> String {
    env::var("CARGO").unwrap_or_else(|_| "cargo".into())
}
fn run(command: &mut Command) -> Result {
    let status = command.current_dir(root()).status()?;
    if !status.success() {
        return Err(format!("Command failed: {command:?} ({status})").into());
    }
    Ok(())
}
fn cargo_run(args: &[&str]) -> Result {
    run(Command::new(cargo()).args(args))
}
// 出力先やtarget tripleを推測せず、このCargo呼出しが返した実行物だけを使う。
#[derive(Default)]
struct BuildArtifacts(BTreeMap<String, PathBuf>);

impl BuildArtifacts {
    fn record(&mut self, message: &Value, package: &str, names: &[&str]) -> Result {
        if message["reason"] != "compiler-artifact"
            || message["package_id"] != package
            || message["profile"]["test"] != false
            || !message["target"]["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
        {
            return Ok(());
        }
        let Some(name) = message["target"]["name"].as_str() else {
            return Ok(());
        };
        if !names.contains(&name) {
            return Ok(());
        }
        let Some(executable) = message["executable"].as_str() else {
            return Ok(());
        };
        let path = PathBuf::from(executable);
        if let Some(previous) = self.0.get(name)
            && previous != &path
        {
            return Err(format!("Cargo returned multiple executables for {name}").into());
        }
        self.0.insert(name.into(), path);
        Ok(())
    }

    fn executable(&self, name: &str) -> Result<&Path> {
        self.0
            .get(name)
            .map(PathBuf::as_path)
            .ok_or_else(|| format!("Cargo did not return an executable for {name}").into())
    }
}

fn read_build_messages(
    mut reader: impl BufRead,
    mut output: impl Write,
    package: &str,
    names: &[&str],
) -> Result<BuildArtifacts> {
    let mut artifacts = BuildArtifacts::default();
    let mut artifact_error = None;
    let mut line = Vec::new();
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        // proc macroなどの通常出力はJSONやUTF-8とは限らない。
        let message = line
            .starts_with(b"{")
            .then(|| serde_json::from_slice::<Value>(&line).ok())
            .flatten();
        let Some(message) = message else {
            // Windowsコンソールのstdoutは有効なUTF-8だけを受け付ける。
            output.write_all(String::from_utf8_lossy(&line).as_bytes())?;
            continue;
        };
        match message["reason"].as_str() {
            Some("compiler-artifact") => {
                if artifact_error.is_none()
                    && let Err(error) = artifacts.record(&message, package, names)
                {
                    // 最後まで読み、後続の通常出力とCargoの終了待ちを維持する。
                    artifact_error = Some(error);
                }
            }
            Some("build-script-executed" | "build-finished") => {}
            _ => output.write_all(&line)?,
        }
    }
    if let Some(error) = artifact_error {
        return Err(error);
    }
    Ok(artifacts)
}

fn cargo_build(args: &[&str], names: &[&str]) -> Result<BuildArtifacts> {
    cargo_build_package("Cargo.toml", args, names, &[])
}

fn cargo_build_package(
    manifest_path: &str,
    args: &[&str],
    names: &[&str],
    environment: &[(&str, &str)],
) -> Result<BuildArtifacts> {
    let mut metadata_command = Command::new(cargo());
    metadata_command
        .current_dir(root())
        .args(["metadata", "--locked", "--no-deps", "--format-version", "1"])
        .stderr(Stdio::inherit());
    let metadata = metadata_command.output()?;
    if !metadata.status.success() {
        return Err(format!("Command failed: {metadata_command:?} ({})", metadata.status).into());
    }
    let metadata: Value = serde_json::from_slice(&metadata.stdout)?;
    let manifest = root().join(manifest_path).canonicalize()?;
    let package = metadata["packages"]
        .as_array()
        .and_then(|packages| {
            packages.iter().find(|package| {
                package["manifest_path"].as_str().is_some_and(|path| {
                    Path::new(path).canonicalize().ok().as_ref() == Some(&manifest)
                })
            })
        })
        .and_then(|package| package["id"].as_str())
        .ok_or("Cargo metadata did not identify the requested package")?;
    let mut command = Command::new(cargo());
    command
        .current_dir(root())
        .args(args)
        .envs(environment.iter().copied())
        // Cargo自身に診断を描画させ、stderr・カラー設定を維持する。
        .arg("--message-format=json-render-diagnostics")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let mut child = command.spawn()?;
    let parsed = read_build_messages(
        BufReader::new(child.stdout.take().unwrap()),
        std::io::stdout(),
        package,
        names,
    );
    let status = child.wait()?;
    if !status.success() {
        return Err(format!("Command failed: {command:?} ({status})").into());
    }
    let artifacts = parsed?;
    for name in names {
        artifacts.executable(name)?;
    }
    Ok(artifacts)
}

fn release_build_args(windows_env: Option<&str>) -> Result<Vec<&'static str>> {
    let mut args = vec!["build", "--locked", "--release", "--bin", "plumivox"];
    if let Some(windows_env) = windows_env {
        let target = match windows_env {
            "msvc" => "x86_64-pc-windows-msvc",
            "gnu" => "x86_64-pc-windows-gnu",
            "gnullvm" => "x86_64-pc-windows-gnullvm",
            _ => return Err("Windows配布ビルドはMSVCまたはGNU環境で実行してください".into()),
        };
        // 配布物の契約はx64。既定targetによる32-bit等の混入を防ぐ。
        args.extend(["--target", target]);
    }
    Ok(args)
}

fn build(sign_windows: bool) -> Result {
    if !cfg!(any(windows, target_os = "macos")) {
        return Err("配布ビルドの対象はWindowsとmacOSです".into());
    }
    fs::create_dir_all(root().join("dist"))?;
    let output = root()
        .join("dist")
        .join(format!("plumivox{}", env::consts::EXE_SUFFIX));
    let windows_env = cfg!(windows).then_some(if cfg!(target_env = "msvc") {
        "msvc"
    } else if cfg!(all(target_env = "gnu", target_abi = "llvm")) {
        "gnullvm"
    } else if cfg!(target_env = "gnu") {
        "gnu"
    } else {
        "unsupported"
    });
    let mut args = release_build_args(windows_env)?;
    if cfg!(target_os = "macos") {
        args.extend(["--target", "aarch64-apple-darwin"]);
    }
    let artifacts = cargo_build(&args, &["plumivox"])?;
    #[cfg(windows)]
    if sign_windows {
        // 検証完了まで既存の配布物を上書きしない。
        windows_signing::sign(artifacts.executable("plumivox")?)?;
    } else {
        println!("Windows: signing skipped (CI preparation artifact)");
    }
    #[cfg(not(windows))]
    let _ = sign_windows;
    fs::copy(artifacts.executable("plumivox")?, &output)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&output, fs::Permissions::from_mode(0o755))?;
    }
    if cfg!(target_os = "macos") {
        run(Command::new("/usr/bin/codesign")
            .args(["--force", "--sign", "-"])
            .arg(&output))?;
    }
    licenses()?;
    println!("Native distribution: {}", output.display());
    Ok(())
}

const VPS_BINARIES: [(&str, &str, u16); 2] = [
    ("x86_64", "plumivox-vps-setup-x86_64", 62),
    ("aarch64", "plumivox-vps-setup-aarch64", 183),
];

// ELF64のprogram headerを読む。PT_INTERPとDT_NEEDEDを拒否し、static PIEも許可する。
fn validate_vps_elf(bytes: &[u8], machine: u16) -> Result {
    let invalid = || "VPS実行物は対象CPUの静的リンクELF64である必要があります";
    let range = |offset: usize, size: usize| -> Result<&[u8]> {
        bytes
            .get(offset..offset.checked_add(size).ok_or_else(invalid)?)
            .ok_or_else(|| invalid().into())
    };
    let number = |offset: usize, size: usize| -> Result<u64> {
        let mut value = [0; 8];
        value[..size].copy_from_slice(range(offset, size)?);
        Ok(u64::from_le_bytes(value))
    };
    if range(0, 7)? != b"\x7fELF\x02\x01\x01"
        || !matches!(number(16, 2)?, 2 | 3)
        || number(18, 2)? != u64::from(machine)
        || number(20, 4)? != 1
        || number(52, 2)? != 64
        || number(54, 2)? != 56
    {
        return Err(invalid().into());
    }
    let offset = usize::try_from(number(32, 8)?)?;
    let count = usize::try_from(number(56, 2)?)?;
    if count == 0 || count == 0xffff {
        return Err(invalid().into());
    }
    range(offset, count.checked_mul(56).ok_or_else(invalid)?)?;
    let mut executable_load = false;
    for index in 0..count {
        let header = offset + index * 56;
        let kind = number(header, 4)?;
        let file_offset = usize::try_from(number(header + 8, 8)?)?;
        let file_size = usize::try_from(number(header + 32, 8)?)?;
        range(file_offset, file_size)?;
        if kind == 3 {
            return Err(invalid().into());
        }
        if kind == 1 && number(header + 4, 4)? & 1 != 0 && file_size > 0 {
            executable_load = true;
        }
        if kind == 2 {
            if file_size % 16 != 0 {
                return Err(invalid().into());
            }
            let mut terminated = false;
            for entry in (file_offset..file_offset + file_size).step_by(16) {
                match number(entry, 8)? {
                    0 => {
                        terminated = true;
                        break;
                    }
                    1 => return Err(invalid().into()),
                    _ => (),
                }
            }
            if !terminated {
                return Err(invalid().into());
            }
        }
    }
    if !executable_load {
        return Err(invalid().into());
    }
    Ok(())
}

fn build_vps(arch: &str) -> Result {
    if !cfg!(target_os = "linux") {
        return Err("VPS配布ビルドはLinuxで実行してください".into());
    }
    let (_, name, machine) = VPS_BINARIES
        .iter()
        .find(|entry| entry.0 == arch)
        .ok_or("VPS配布対象はx86_64またはaarch64です")?;
    let triple = format!("{arch}-unknown-linux-musl");
    let key = format!(
        "CARGO_TARGET_{}_",
        triple.to_ascii_uppercase().replace('-', "_")
    );
    let linker_key = format!("{key}LINKER");
    let flags_key = format!("{key}RUSTFLAGS");
    let artifacts = cargo_build_package(
        "vps-installer/Cargo.toml",
        &[
            "build",
            "--locked",
            "--release",
            "--package",
            "plumivox-vps-installer",
            "--bin",
            "plumivox-vps-setup",
            "--target",
            &triple,
        ],
        &["plumivox-vps-setup"],
        &[
            (&linker_key, "rust-lld"),
            (
                &flags_key,
                "-C link-self-contained=yes -C target-feature=+crt-static",
            ),
        ],
    )?;
    let executable = artifacts.executable("plumivox-vps-setup")?;
    validate_vps_elf(&fs::read(executable)?, *machine)?;
    fs::copy(executable, root().join("setup/vps").join(name))?;
    licenses()?;
    Ok(())
}

fn check() -> Result {
    cargo_run(&["fmt", "--all", "--", "--check"])?;
    cargo_run(&[
        "check",
        "--locked",
        "--workspace",
        "--all-targets",
        "--features",
        "test-fixture",
    ])?;
    for file in [
        "plugin.json",
        "mcp.json",
        ".agents/plugins/marketplace.json",
    ] {
        serde_json::from_slice::<Value>(&fs::read(root().join(file))?)?;
    }
    let installer = root()
        .join("setup/vps/install.sh")
        .to_string_lossy()
        .replace('\\', "/");
    // Windowsの PATH 上の `bash` は WSL の起動スタブ（配布が無いと起動できても失敗する）を指し得る。
    // 構文検査はGit for Windowsの bash を優先し、無い環境だけ PATH の bash を使う。
    let git_bash = Path::new("C:/Program Files/Git/bin/bash.exe");
    let program = if cfg!(windows) && git_bash.exists() {
        git_bash.as_os_str()
    } else {
        OsStr::new("bash")
    };
    let mut bash = Command::new(program);
    bash.args(["-n", &installer]).current_dir(root());
    match bash.status() {
        Ok(status) if status.success() => (),
        other => return Err(format!("Bash syntax check failed: {other:?}").into()),
    }
    for file in [
        "setup/vps/squid.conf.in",
        "setup/vps/plumivox-proxy.service",
        "skills/setup-plumivox/SKILL.md",
    ] {
        fs::read(root().join(file))?;
    }
    for (_, name, machine) in VPS_BINARIES {
        let path = root().join("setup/vps").join(name);
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            let reason = if error.kind() == std::io::ErrorKind::NotFound {
                "VPS実行物がありません"
            } else {
                "VPS実行物を確認できません"
            };
            format!("{reason}: {}", path.display())
        })?;
        if !metadata.file_type().is_file() {
            return Err("VPS実行物は通常ファイルである必要があります".into());
        }
        validate_vps_elf(&fs::read(&path)?, machine)?;
    }
    for file in fs::read_dir(root().join("setup/vps"))? {
        let path = file?.path();
        if VPS_BINARIES
            .iter()
            .any(|entry| path.file_name().is_some_and(|name| name == entry.1))
        {
            continue;
        }
        if fs::read_to_string(&path)?.contains('\r') {
            return Err(format!("VPS配布資材はLFで保存してください: {}", path.display()).into());
        }
    }
    cargo_run(&["test", "--locked", "--package", "xtask"])?;
    println!("Rust・配布資材・3設定ファイル 合格");
    Ok(())
}

fn normalize_license_newlines(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn licenses() -> Result {
    let output = Command::new(cargo())
        .args(["metadata", "--locked", "--format-version", "1"])
        .current_dir(root())
        .stderr(Stdio::inherit())
        .output()?;
    if !output.status.success() {
        return Err("Cargo metadata unavailable".into());
    }
    let metadata: Value = serde_json::from_slice(&output.stdout)?;
    let mut packages = metadata["packages"]
        .as_array()
        .ok_or("metadata packages missing")?
        .iter()
        .filter(|p| {
            p["name"] != "plumivox" && p["name"] != "xtask" && p["name"] != "plumivox-vps-installer"
        })
        .collect::<Vec<_>>();
    packages.sort_by_key(|p| {
        (
            p["name"].as_str().unwrap_or(""),
            p["version"].as_str().unwrap_or(""),
        )
    });
    let mut sections =
        vec!["Plumivox native dependency licenses\nGenerated from Cargo.lock.".to_string()];
    let pattern = regex::Regex::new(r"(?i)^(LICENSE|LICENCE|COPYING|NOTICE)([._-]|$)")?;
    for package in packages {
        let directory = Path::new(
            package["manifest_path"]
                .as_str()
                .ok_or("manifest path missing")?,
        )
        .parent()
        .unwrap();
        let mut names = fs::read_dir(directory)?
            .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect::<std::io::Result<Vec<_>>>()?
            .into_iter()
            .filter(|n| pattern.is_match(n))
            .collect::<Vec<_>>();
        if let Some(name) = package["license_file"].as_str() {
            if !names.iter().any(|n| n == name) {
                names.push(name.into());
            }
        }
        names.sort();
        let mut texts = Vec::new();
        for name in names {
            let path = directory.join(&name);
            if path.is_file() {
                texts.push(format!("{name}\n{}", fs::read_to_string(path)?));
            }
        }
        sections.push(format!(
            "{} {} ({})\n{}",
            package["name"].as_str().unwrap(),
            package["version"].as_str().unwrap(),
            package["license"].as_str().unwrap_or("see license file"),
            texts.join("\n\n")
        ));
    }
    sections.push(format!(
        "emoji-regex 11.0.0 (MIT)\n{}",
        normalize_license_newlines(&fs::read_to_string(
            root().join("third-party/emoji-regex/LICENSE-MIT.txt")
        )?)
    ));
    fs::create_dir_all(root().join("dist"))?;
    let text =
        sections.join("\n\n============================================================\n\n");
    let text = regex::Regex::new(r"(?m)[ \t]+\r?$")?.replace_all(&text, "");
    fs::write(
        root().join("dist/THIRD-PARTY-LICENSES.txt"),
        text.as_bytes(),
    )?;
    Ok(())
}

fn e2e(command: &str) -> Result {
    if command == "test-e2e" && env::var_os("PLUMIVOX_E2E_CASE").is_some_and(|s| !s.is_empty()) {
        return Err("全件検証ではPLUMIVOX_E2E_CASEを未設定にしてください".into());
    }
    if command == "test-setup-e2e" && !cfg!(any(windows, target_os = "macos")) {
        return Err("PCセットアップE2EはWindowsまたはmacOSで実行してください".into());
    }
    // 未知の test-* は、数分かかる fixture ビルドと lib テストの前に拒否する。
    let targets: &[&str] = match command {
        "test-e2e" => &[
            "native_e2e",
            "tls_e2e",
            "api_e2e",
            "proxy_e2e",
            "setup_e2e",
            "setup_vps_e2e",
        ],
        "test-api-e2e" => &["api_e2e"],
        "test-proxy-e2e" | "test-proxy-shared-e2e" => &["proxy_e2e"],
        "test-setup-e2e" => &["setup_e2e", "setup_vps_e2e"],
        _ => return Err("unknown E2E command".into()),
    };
    let artifacts = cargo_build(
        &[
            "build",
            "--locked",
            "--features",
            "test-fixture",
            "--bin",
            "plumivox-fixture",
            "--bin",
            "plumivox",
        ],
        &["plumivox-fixture", "plumivox"],
    )?;
    if matches!(
        command,
        "test-e2e" | "test-proxy-e2e" | "test-proxy-shared-e2e"
    ) {
        let mut args = vec!["test", "--locked", "--lib"];
        if command != "test-e2e" {
            args.push("proxy");
        }
        if cfg!(target_os = "macos") {
            args.extend(["--features", "test-fixture"]);
        }
        cargo_run(&args)?;
    }
    for target in targets {
        if *target == "setup_e2e" && !cfg!(any(windows, target_os = "macos")) {
            println!("PCセットアップE2E未実行: 対象OS外");
            continue;
        }
        run(Command::new(cargo())
            .args([
                "test",
                "--locked",
                "--features",
                "test-fixture",
                "--test",
                target,
                "--",
                "--nocapture",
            ])
            .env(
                "PLUMIVOX_NATIVE_FIXTURE",
                artifacts.executable("plumivox-fixture")?,
            )
            .env("PLUMIVOX_NATIVE_BINARY", artifacts.executable("plumivox")?))?;
    }
    Ok(())
}

fn main() -> Result {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if matches!(
        args.first().map(String::as_str),
        Some("artifact-upload" | "artifact-download")
    ) {
        return r2::execute(&args);
    }
    if args.first().map(String::as_str) == Some("build-vps") {
        if args.len() != 2 {
            return Err("cargo xtask build-vps <x86_64|aarch64>".into());
        }
        return build_vps(&args[1]);
    }
    match args.first().map(String::as_str) {
        Some("build") if args.len() == 1 => build(true),
        Some("build-unsigned") if args.len() == 1 => build(false),
        Some("check") => check(),
        Some("licenses") => licenses(),
        Some(command) if command.starts_with("ci-") => ci::execute(command),
        Some(command) if command.starts_with("test-") => e2e(command),
        _ => Err(USAGE.into()),
    }
}

/// 引数なし・不明なコマンドで示す一覧。main の分岐と r2・ci の入口を合わせて載せる。
const USAGE: &str = "cargo xtask build | build-unsigned | build-vps <x86_64|aarch64> | check | licenses | test-e2e | test-api-e2e | test-proxy-e2e | test-proxy-shared-e2e | test-setup-e2e | artifact-upload | artifact-download | ci-<command>";

#[cfg(test)]
mod artifact_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn license_text_is_independent_of_checkout_newlines() {
        assert_eq!(
            normalize_license_newlines("first\r\nsecond\r\n"),
            normalize_license_newlines("first\nsecond\n")
        );
    }

    fn elf(machine: u16) -> Vec<u8> {
        let mut bytes = vec![0; 192];
        bytes[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        bytes[16..18].copy_from_slice(&2u16.to_le_bytes());
        bytes[18..20].copy_from_slice(&machine.to_le_bytes());
        bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
        bytes[32..40].copy_from_slice(&64u64.to_le_bytes());
        bytes[52..54].copy_from_slice(&64u16.to_le_bytes());
        bytes[54..56].copy_from_slice(&56u16.to_le_bytes());
        bytes[56..58].copy_from_slice(&1u16.to_le_bytes());
        bytes[64..68].copy_from_slice(&1u32.to_le_bytes());
        bytes[68..72].copy_from_slice(&5u32.to_le_bytes());
        bytes[96..104].copy_from_slice(&192u64.to_le_bytes());
        bytes
    }

    #[test]
    fn vps_elf_requires_expected_machine_and_no_dynamic_loader_or_dependencies() {
        for machine in [62, 183] {
            let bytes = elf(machine);
            validate_vps_elf(&bytes, machine).unwrap();
            assert!(validate_vps_elf(&bytes, if machine == 62 { 183 } else { 62 }).is_err());
            assert!(validate_vps_elf(&bytes[..100], machine).is_err());
            let mut dynamic = bytes.clone();
            dynamic[16..18].copy_from_slice(&3u16.to_le_bytes());
            dynamic[56..58].copy_from_slice(&2u16.to_le_bytes());
            dynamic[120..124].copy_from_slice(&2u32.to_le_bytes());
            dynamic[128..136].copy_from_slice(&176u64.to_le_bytes());
            dynamic[152..160].copy_from_slice(&16u64.to_le_bytes());
            validate_vps_elf(&dynamic, machine).unwrap();
            dynamic[176..184].copy_from_slice(&1u64.to_le_bytes());
            assert!(validate_vps_elf(&dynamic, machine).is_err());
            dynamic[120..124].copy_from_slice(&3u32.to_le_bytes());
            assert!(validate_vps_elf(&dynamic, machine).is_err());
            let mut overflowing = bytes;
            overflowing[32..40].copy_from_slice(&u64::MAX.to_le_bytes());
            assert!(validate_vps_elf(&overflowing, machine).is_err());
        }
    }

    fn artifact(name: &str, executable: &str) -> Value {
        // Cargoのcompiler-artifact形式。freshでも実行物が通知される。
        serde_json::from_str(&format!(
            r#"{{"reason":"compiler-artifact","package_id":"path+file:///repo#plumivox@1.0.7","target":{{"kind":["bin"],"name":"{name}"}},"profile":{{"test":false}},"filenames":[],"executable":{},"fresh":true}}"#,
            serde_json::to_string(executable).unwrap()
        ))
        .unwrap()
    }

    const PACKAGE: &str = "path+file:///repo#plumivox@1.0.7";
    const NAMES: &[&str] = &["plumivox", "plumivox-fixture"];

    #[test]
    fn windows_distribution_explicitly_selects_x64_for_each_supported_abi() {
        for (abi, target) in [
            ("msvc", "x86_64-pc-windows-msvc"),
            ("gnu", "x86_64-pc-windows-gnu"),
            ("gnullvm", "x86_64-pc-windows-gnullvm"),
        ] {
            let args = release_build_args(Some(abi)).unwrap();
            assert_eq!(
                args,
                [
                    "build",
                    "--locked",
                    "--release",
                    "--bin",
                    "plumivox",
                    "--target",
                    target
                ]
            );
        }
        assert!(release_build_args(Some("unsupported")).is_err());
        assert_eq!(
            release_build_args(None).unwrap(),
            ["build", "--locked", "--release", "--bin", "plumivox"]
        );
    }

    #[test]
    fn preserves_stdout_after_arbitrary_tool_output_without_rendering_diagnostics() {
        let ordinary = b"plain output\r\n{not json}\n{\xff}\n\xff\n{\"tool\":true}\n{\"reason\":\"unknown-cargo-message\"}\n";
        let mut stream = ordinary.to_vec();
        // json-render-diagnosticsでは実際の診断はCargoがstderrへ描画する。
        // 同名reasonを持つ通常JSON出力を手動の診断と誤認しない。
        let ordinary_json =
            json!({"reason":"compiler-message", "message":{"rendered":"ordinary JSON data"}});
        for message in [
            artifact("plumivox", "actual/x64/release/plumivox.exe"),
            artifact("plumivox-fixture", "actual/x64/debug/plumivox-fixture.exe"),
            ordinary_json.clone(),
            json!({"reason":"build-script-executed", "package_id":PACKAGE}),
            json!({"reason":"build-finished", "success":true}),
        ] {
            stream.extend(serde_json::to_vec(&message).unwrap());
            stream.push(b'\n');
        }
        // 最後の改行がなくても通常出力を失わない。
        stream.extend(b"{trailing output");
        let mut output = Vec::new();
        let artifacts = read_build_messages(&stream[..], &mut output, PACKAGE, NAMES).unwrap();
        let mut expected = String::from_utf8_lossy(ordinary).as_bytes().to_vec();
        expected.extend(serde_json::to_vec(&ordinary_json).unwrap());
        expected.extend(b"\n{trailing output");
        assert_eq!(output, expected);
        assert_eq!(
            artifacts.executable("plumivox").unwrap(),
            Path::new("actual/x64/release/plumivox.exe")
        );
        assert_eq!(
            artifacts.executable("plumivox-fixture").unwrap(),
            Path::new("actual/x64/debug/plumivox-fixture.exe")
        );
    }

    #[test]
    fn drains_stdout_after_ambiguous_artifacts() {
        let mut stream = Vec::new();
        for message in [
            artifact("plumivox", "first/plumivox"),
            artifact("plumivox", "second/plumivox"),
            json!({"late":"ordinary output"}),
        ] {
            stream.extend(serde_json::to_vec(&message).unwrap());
            stream.push(b'\n');
        }
        stream.extend(b"late non-JSON stdout\n");
        let mut output = Vec::new();
        let mut cursor = std::io::Cursor::new(&stream);
        assert!(read_build_messages(&mut cursor, &mut output, PACKAGE, NAMES).is_err());
        assert_eq!(cursor.position(), stream.len() as u64);
        assert_eq!(
            output,
            b"{\"late\":\"ordinary output\"}\nlate non-JSON stdout\n"
        );
    }

    #[test]
    fn ignores_other_packages_test_harnesses_and_non_executables() {
        let message = artifact("plumivox", "old-target/release/plumivox");
        let mut artifacts = BuildArtifacts::default();
        let mut other_package = message.clone();
        other_package["package_id"] = json!("registry+https://example.invalid#plumivox@1.0.7");
        let mut harness = message.clone();
        harness["profile"]["test"] = json!(true);
        let mut library = message.clone();
        library["target"]["kind"] = json!(["lib"]);
        let mut no_executable = message.clone();
        no_executable["executable"] = Value::Null;
        let mut other_binary = message;
        other_binary["target"]["name"] = json!("unrelated");
        for ignored in [other_package, harness, library, no_executable, other_binary] {
            artifacts.record(&ignored, PACKAGE, NAMES).unwrap();
        }
        assert!(artifacts.executable("plumivox").is_err());
        assert!(artifacts.executable("plumivox-fixture").is_err());
    }

    #[test]
    fn refuses_ambiguous_target_outputs_and_keeps_builds_separate() {
        let arm = artifact("plumivox", "custom/aarch64-apple-darwin/release/plumivox");
        let other = artifact("plumivox", "other/aarch64-apple-darwin/release/plumivox");
        let mut first = BuildArtifacts::default();
        first.record(&arm, PACKAGE, NAMES).unwrap();
        first.record(&arm, PACKAGE, NAMES).unwrap();
        assert!(first.record(&other, PACKAGE, NAMES).is_err());
        let mut second = BuildArtifacts::default();
        second.record(&other, PACKAGE, NAMES).unwrap();
        assert_ne!(
            first.executable("plumivox").unwrap(),
            second.executable("plumivox").unwrap()
        );
    }
}
