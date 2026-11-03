mod automerge;
mod r2;

use serde_json::Value;
use std::{
    env,
    error::Error,
    fs,
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
fn target() -> PathBuf {
    let p = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("target"));
    if p.is_absolute() { p } else { root().join(p) }
}
fn executable(name: &str) -> PathBuf {
    target()
        .join("debug")
        .join(format!("{name}{}", env::consts::EXE_SUFFIX))
}

fn build(universal: bool) -> Result {
    if universal && !cfg!(target_os = "macos") {
        return Err("Universal builds require macOS".into());
    }
    if !cfg!(any(windows, target_os = "macos")) {
        return Err("配布ビルドの対象はWindowsとmacOSです".into());
    }
    fs::create_dir_all(root().join("dist"))?;
    let output = root()
        .join("dist")
        .join(format!("plumivox{}", env::consts::EXE_SUFFIX));
    if universal {
        let arches = ["aarch64-apple-darwin", "x86_64-apple-darwin"];
        for arch in arches {
            cargo_run(&[
                "build",
                "--locked",
                "--release",
                "--bin",
                "plumivox",
                "--target",
                arch,
            ])?;
        }
        let mut cmd = Command::new("/usr/bin/lipo");
        cmd.arg("-create");
        for arch in arches {
            cmd.arg(target().join(arch).join("release/plumivox"));
        }
        run(cmd.arg("-output").arg(&output))?;
    } else {
        cargo_run(&["build", "--locked", "--release", "--bin", "plumivox"])?;
        fs::copy(
            target().join("release").join(output.file_name().unwrap()),
            &output,
        )?;
    }
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
    if cfg!(windows) {
        let script = root()
            .join("setup/Setup-Plumivox.ps1")
            .to_string_lossy()
            .replace('\'', "''");
        let expression = format!(
            "$tokens=$null;$errors=$null;[void][System.Management.Automation.Language.Parser]::ParseFile('{script}',[ref]$tokens,[ref]$errors);if($errors.Count){{$errors|ForEach-Object{{[Console]::Error.WriteLine($_.Message)}};exit 1}}"
        );
        run(Command::new("pwsh").args(["-NoProfile", "-NonInteractive", "-Command", &expression]))?;
    }
    let installer = root()
        .join("setup/vps/install.sh")
        .to_string_lossy()
        .replace('\\', "/");
    let mut bash = Command::new("bash");
    bash.args(["-n", &installer]).current_dir(root());
    match bash.status() {
        Ok(status) if status.success() => (),
        Err(_) if cfg!(windows) => {
            run(Command::new("C:/Program Files/Git/bin/bash.exe").args(["-n", &installer]))?
        }
        other => return Err(format!("Bash syntax check failed: {other:?}").into()),
    }
    for file in [
        "setup/vps/squid.conf.in",
        "setup/vps/plumivox-proxy.service",
        "skills/setup-plumivox/SKILL.md",
    ] {
        fs::read(root().join(file))?;
    }
    for file in fs::read_dir(root().join("setup/vps"))? {
        let path = file?.path();
        if fs::read_to_string(&path)?.contains('\r') {
            return Err(format!("VPS配布資材はLFで保存してください: {}", path.display()).into());
        }
    }
    cargo_run(&["test", "--locked", "--package", "xtask"])?;
    println!("Rust・セットアップ構文・3設定ファイル 合格");
    Ok(())
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
        .filter(|p| p["name"] != "plumivox" && p["name"] != "xtask")
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
        fs::read_to_string(root().join("third-party/emoji-regex/LICENSE-MIT.txt"))?
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
    cargo_run(&[
        "build",
        "--locked",
        "--features",
        "test-fixture",
        "--bin",
        "plumivox-fixture",
        "--bin",
        "plumivox",
    ])?;
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
            .env("PLUMIVOX_NATIVE_FIXTURE", executable("plumivox-fixture"))
            .env("PLUMIVOX_NATIVE_BINARY", executable("plumivox")))?;
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
    match args.first().map(String::as_str) {Some("build")=>build(args.iter().any(|a|a=="--universal")),Some("check")=>check(),Some("licenses")=>licenses(),Some("dependabot-auto-merge")=>automerge::execute(),Some(command) if command.starts_with("test-")=>e2e(command),_=>Err("cargo xtask build [--universal] | check | licenses | test-e2e | test-api-e2e | test-proxy-e2e | test-proxy-shared-e2e | test-setup-e2e | dependabot-auto-merge".into())}
}
