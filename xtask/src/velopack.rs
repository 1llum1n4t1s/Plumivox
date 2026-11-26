//! Setup（Velopack）の梱包・公開・隔離E2E。
//!
//! AGENTS.md の「CI の手順は Rust、PowerShell スクリプトを使わない」に従い、Remnifold の
//! `scripts/velopack/pack-windows.ps1`・`publish-r2.ps1` に相当する処理をここで行う。外部ツールは
//! Velopack CLI（`vpk` 1.3.39-g8d2844f）、署名の Sigilune、R2 転送の curl だけを呼ぶ。
//!
//! - `velopack-pack`: `vpk pack` で Setup・`releases.win.json`・full nupkg を作る。`--sign` は Sigilune の
//!   署名テンプレートで全 PE と Setup に署名し、署名後に nupkg 内の全 PE と Setup の署名者を検証する
//!   （アプリが適用前に行う検査に通らない一式は配信しない）。`--binary` を渡すと cargo を動かさない
//!   （CI では署名用 Secrets を渡す step で cargo・build script を動かさないため、先の step のビルド物を渡す）。
//! - `velopack-publish`: 公開バケット `plumivox-updates` の `velopack/` へ置く。版付きの成果物は新規作成のみ
//!   （既存なら内容を照合し、違えば失敗）、固定名（最新を指す Setup・`releases.<channel>.json` など）は
//!   置き換え、`releases.<channel>.json` を最後に置く。
//! - `test-setup-velopack-e2e`: テスト用 packId で未署名の 9.0.0・9.0.1 を梱包し、
//!   `tests/setup_velopack_e2e.rs` を起動する（本物の導入・登録・Run キーの値には触れない）。
use crate::{
    Result, cargo_build, cargo_build_package, quiet,
    r2::{MAX_FILE, PutMode, Store},
    root,
};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

/// 本番の packId（`Plumivox` は既存の `%LOCALAPPDATA%\Plumivox` と衝突するため使わない）。
pub(crate) const PROD_PACK_ID: &str = "PlumivoxApp";
/// 隔離E2E用の packId。本番の Run 値・Uninstall の項目・導入先と衝突しない。
pub(crate) const TEST_PACK_ID: &str = "PlumivoxVelopackTest";
/// 要求する Velopack CLI の版（crate の `velopack = "=1.3.39-g8d2844f"` と揃える）。
const VPK_VERSION: &str = "1.3.39-g8d2844f";
/// 公開バケットの名前と、その中の配信フォルダー（アプリの FEED_URL は `https://plumivox-updates.kagayoi.com/velopack`）。
const UPDATE_BUCKET: &str = "plumivox-updates";
const FEED_PREFIX: &str = "velopack/";
/// 公開用の資格情報の環境変数の接頭辞（CI 成果物用の `PLUMIVOX_R2_` とは別の鍵を使う）。
const UPDATE_ENV_PREFIX: &str = "PLUMIVOX_UPDATES_R2_";
/// `update-feed-test` 機能付きビルドにだけ含まれる文字列。署名梱包でテスト用ビルドを拒否するために使う。
const TEST_BUILD_MARKER: &[u8] = b"PLUMIVOX_UPDATE_FEED_DIR";
/// E2E で梱包する2つの版（9.0.0 を導入し、9.0.1 へ更新する）。
const E2E_VERSIONS: [&str; 2] = ["9.0.0", "9.0.1"];

pub fn execute(args: &[String]) -> Result {
    match args.first().map(String::as_str) {
        Some("velopack-pack") => {
            let options = PackOptions::parse(&args[1..])?;
            pack(&options)
        }
        Some("velopack-publish") => {
            let options = PublishOptions::parse(&args[1..])?;
            publish(&options)
        }
        Some("test-setup-velopack-e2e") => e2e(&args[1..]),
        _ => Err("unknown Velopack command".into()),
    }
}

/// `--name value` と `--flag` の組を読む。`flags` は値を取らない名前。重複と未知の名前は拒否する。
fn parse_options(
    args: &[String],
    valued: &[&str],
    flags: &[&str],
) -> Result<std::collections::BTreeMap<String, Option<String>>> {
    let mut options = std::collections::BTreeMap::new();
    let mut index = 0;
    while index < args.len() {
        let name = args[index].as_str();
        let value = if flags.contains(&name) {
            None
        } else if valued.contains(&name) {
            index += 1;
            Some(
                args.get(index)
                    .filter(|value| !value.is_empty() && !value.starts_with("--"))
                    .ok_or_else(|| format!("{name}に値が必要です"))?
                    .clone(),
            )
        } else {
            return Err(format!("未知のオプションです: {name}").into());
        };
        if options.insert(name.to_string(), value).is_some() {
            return Err(format!("{name}が重複しています").into());
        }
        index += 1;
    }
    Ok(options)
}

fn absolute(value: &str, label: &str) -> Result<PathBuf> {
    let path = PathBuf::from(value);
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(format!("{label}には親参照を含まない絶対パスを指定してください").into());
    }
    Ok(path)
}

fn valid_pack_id(id: &str) -> bool {
    id.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && id.chars().all(|c| c.is_ascii_alphanumeric())
}

fn valid_version(version: &str) -> bool {
    let parts: Vec<_> = version.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|b| b.is_ascii_digit())
                && (part.len() == 1 || !part.starts_with('0'))
        })
}

/// `Cargo.toml` の `[package]` の版（cargo を動かさずに読む。署名 step でも使えるようにするため）。
pub(crate) fn package_version(manifest: &str) -> Result<String> {
    let pattern = regex::Regex::new(r#"(?ms)^\[package\]\s*$.*?^version\s*=\s*"([^"]+)"\s*$"#)?;
    let version = pattern
        .captures(manifest)
        .and_then(|captures| captures.get(1))
        .map(|found| found.as_str().to_string())
        .ok_or("Cargo.tomlの[package]のversionを読めません")?;
    if !valid_version(&version) {
        return Err("Cargo.tomlのversionがx.y.zではありません".into());
    }
    Ok(version)
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PackOptions {
    /// 空の作業フォルダー（payload・packages・temp を作る）。
    pub(crate) work: PathBuf,
    /// Sigilune で署名し、署名者を検証する（本番の packId・Cargo の版だけ）。
    pub(crate) sign: bool,
    pub(crate) pack_id: String,
    /// 省略時は Cargo.toml の版。
    pub(crate) version: Option<String>,
    /// ビルド済みの plumivox.exe。指定すると cargo を動かさない。
    pub(crate) binary: Option<PathBuf>,
}

impl PackOptions {
    fn parse(args: &[String]) -> Result<Self> {
        let options = parse_options(
            args,
            &["--work", "--pack-id", "--version", "--binary"],
            &["--sign"],
        )?;
        let value = |name: &str| options.get(name).cloned().flatten();
        let parsed = Self {
            work: absolute(
                &value("--work").ok_or("--work（空の作業フォルダー）が必要です")?,
                "--work",
            )?,
            sign: options.contains_key("--sign"),
            pack_id: value("--pack-id").unwrap_or_else(|| PROD_PACK_ID.into()),
            version: value("--version"),
            binary: value("--binary")
                .map(|binary| absolute(&binary, "--binary"))
                .transpose()?,
        };
        parsed.validate()?;
        Ok(parsed)
    }

    /// 組合せの検査。署名はテスト用の packId・版と併用できない（テスト用の一式に本番の署名をしない）。
    fn validate(&self) -> Result {
        if !valid_pack_id(&self.pack_id) {
            return Err("packIdは英字で始まる英数字にしてください".into());
        }
        if let Some(version) = &self.version
            && !valid_version(version)
        {
            return Err("--versionはx.y.zで指定してください".into());
        }
        if self.sign && (self.pack_id != PROD_PACK_ID || self.version.is_some()) {
            return Err("--signは--pack-id・--version（隔離検証用）と併用できません".into());
        }
        if !self.sign && self.pack_id == PROD_PACK_ID && self.version.is_none() {
            // 本番の packId の未署名梱包は、配信すると全利用者の適用前の検査で拒否される一式になる。
            println!("注意: 本番packIdを未署名で梱包します（配信には使えません）");
        }
        Ok(())
    }

    fn is_test_package(&self) -> bool {
        self.pack_id != PROD_PACK_ID
    }
}

/// 作業フォルダーが、リンク（reparse point）を含まない既存の空フォルダーか。
fn require_empty_plain_directory(work: &Path) -> Result {
    let metadata = fs::symlink_metadata(work)
        .map_err(|_| "作業フォルダーは既存の空フォルダーにしてください")?;
    if !metadata.is_dir() || fs::read_dir(work)?.next().is_some() {
        return Err("作業フォルダーは既存の空フォルダーにしてください".into());
    }
    for ancestor in work.ancestors() {
        let metadata = fs::symlink_metadata(ancestor)?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(
                    format!("作業パスにreparse pointがあります: {}", ancestor.display()).into(),
                );
            }
        }
        if metadata.file_type().is_symlink() {
            return Err(format!("作業パスにリンクがあります: {}", ancestor.display()).into());
        }
    }
    let repository = root();
    if work.starts_with(&repository) {
        return Err("作業フォルダーはリポジトリの外にしてください".into());
    }
    Ok(())
}

/// vpk の版を確かめる（`--version` は無いため `--help` の見出しで照合する。色付けの制御文字は除く）。
fn require_vpk() -> Result {
    let output = quiet(Command::new("vpk").arg("--help"))
        .output()
        .map_err(|_| "Velopack CLI（vpk）が見つかりません。dotnet tool install --global vpk --version 1.3.39-g8d2844f で導入してください")?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned()
        + &String::from_utf8_lossy(&output.stderr);
    let plain = regex::Regex::new("\u{1b}\\[[0-9;]*m")?.replace_all(&text, "");
    if !regex::Regex::new(&format!(r"\bVelopack CLI {}\b", regex::escape(VPK_VERSION)))?
        .is_match(&plain)
    {
        return Err(format!("Velopack CLI {VPK_VERSION} が必要です").into());
    }
    Ok(())
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// nupkg の中の名前を確かめ、署名を検査すべき PE（.exe・.dll）の名前を返す。アプリの
/// `src/app_update/trust.rs` と同じ条件（曖昧な名前の拒否、主実行ファイルと Squirrel.exe がちょうど1つずつ）。
pub(crate) fn package_pe_names(names: &[String]) -> Result<Vec<String>> {
    let (mut main, mut updater) = (0, 0);
    let mut pes = Vec::new();
    for name in names {
        if name.contains(['\\', ':'])
            || name.starts_with('/')
            || name.split('/').any(|part| part == "..")
        {
            return Err(format!("nupkgに許可されない名前があります: {name}").into());
        }
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".exe") || lower.ends_with(".dll") {
            pes.push(name.clone());
        }
        main += usize::from(name.eq_ignore_ascii_case("lib/app/plumivox.exe"));
        updater += usize::from(name.eq_ignore_ascii_case("lib/app/Squirrel.exe"));
    }
    if main != 1 || updater != 1 {
        return Err(
            "nupkgにはlib/app/plumivox.exeとlib/app/Squirrel.exeがちょうど1つずつ必要です".into(),
        );
    }
    Ok(pes)
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// 梱包する。出力は `<work>/packages`（Setup・版付き Setup・`releases.win.json`・full nupkg など）。
pub(crate) fn pack(options: &PackOptions) -> Result {
    if !cfg!(windows) {
        return Err("Windows版のSetupはWindows x64で梱包してください".into());
    }
    require_empty_plain_directory(&options.work)?;
    require_vpk()?;
    let version = match &options.version {
        Some(version) => version.clone(),
        None => package_version(&fs::read_to_string(root().join("Cargo.toml"))?)?,
    };
    let payload = options.work.join("payload");
    let packages = options.work.join("packages");
    let temp = options.work.join("temp");
    for directory in [&payload, &packages, &temp] {
        fs::create_dir(directory)?;
    }
    // 実行ファイル。--binary が無ければ既定の機能だけで release ビルドする（update-feed-test を含めない）。
    let binary = match &options.binary {
        Some(binary) => {
            if !fs::symlink_metadata(binary).is_ok_and(|meta| meta.is_file()) {
                return Err("--binaryには既存のplumivox.exeを指定してください".into());
            }
            binary.clone()
        }
        None => cargo_build(
            &[
                "build",
                "--locked",
                "--release",
                "--bin",
                "plumivox",
                "--target",
                "x86_64-pc-windows-msvc",
            ],
            &["plumivox"],
        )?
        .executable("plumivox")?
        .to_path_buf(),
    };
    let bytes = fs::read(&binary)?;
    if options.sign && contains_bytes(&bytes, TEST_BUILD_MARKER) {
        return Err("update-feed-test機能付きのビルドには署名できません".into());
    }
    fs::write(payload.join("plumivox.exe"), &bytes)?;
    for name in ["LICENSE", "README.md", "CHANGELOG.md"] {
        fs::copy(root().join(name), payload.join(name))?;
    }
    let licenses = root().join("dist").join("THIRD-PARTY-LICENSES.txt");
    if licenses.is_file() {
        fs::copy(&licenses, payload.join("THIRD-PARTY-LICENSES.txt"))?;
    } else if options.sign {
        return Err("dist/THIRD-PARTY-LICENSES.txtがありません（先に cargo xtask licenses を実行してください）".into());
    }
    let test = options.is_test_package();
    let mut arguments: Vec<String> = [
        "pack",
        "--packId",
        &options.pack_id,
        // テスト用は題名も変え、万一ショートカットや一覧に出ても本番と取り違えないようにする。
        "--packTitle",
        if test {
            "Plumivox Velopack Test"
        } else {
            "Plumivox"
        },
        "--packAuthors",
        "Kagayoi",
        "--packVersion",
        &version,
        "--channel",
        "win",
        "--runtime",
        "win-x64",
        "--instLocation",
        "PerUser",
        "--mainExe",
        "plumivox.exe",
        "--packDir",
        payload.to_str().ok_or("作業パスがUTF-8ではありません")?,
        "--outputDir",
        packages.to_str().ok_or("作業パスがUTF-8ではありません")?,
        // 本番はスタートメニューだけ（引数なしの起動は案内を表示する）。テスト用は本物のスタートメニュー・
        // デスクトップへショートカットを作らない（Velopack はショートカットを既知フォルダーへ作り、
        // 環境変数の差し替えでは隔離できないため）。
        "--shortcuts",
        if test { "" } else { "StartMenuRoot" },
        "--noPortable",
        "--delta",
        "None",
    ]
    .iter()
    .map(|value| value.to_string())
    .collect();
    #[cfg(windows)]
    if options.sign {
        let sigilune = crate::windows_signing::sigilune()?;
        // 梱包の前にログインし、認証の失敗を早く検出する（有効なセッションは再利用する）。
        crate::run(Command::new(&sigilune).arg("login"))?;
        // Velopack はファイルごとに署名テンプレートを実行する。1 ファイルずつ直列に署名し、
        // Sigilune が署名後に有効性・署名者・タイムスタンプを検証する。--replace は既存の署名を置き換える。
        arguments.extend([
            "--signParallel".to_string(),
            "1".to_string(),
            "--signTemplate".to_string(),
            format!(
                "\"{}\" sign --replace --description Plumivox {{{{file}}}}",
                sigilune.display()
            ),
        ]);
    }
    // vpk とその子の一時ファイルを、梱包専用のフォルダーに閉じる。
    let status = quiet(
        Command::new("vpk")
            .args(&arguments)
            .env("TEMP", &temp)
            .env("TMP", &temp)
            .current_dir(&options.work),
    )
    .status()?;
    if !status.success() {
        return Err(format!("vpk packが失敗しました ({status})").into());
    }
    let setup = packages.join(format!("{}-win-Setup.exe", options.pack_id));
    let nupkg = packages.join(format!("{}-{version}-full.nupkg", options.pack_id));
    for required in [&setup, &nupkg, &packages.join("releases.win.json")] {
        if !required.is_file() {
            return Err(format!("梱包の出力がありません: {}", required.display()).into());
        }
    }
    // 版付きの不変名の Setup も置く（固定名の Setup は最新版を指す URL として毎回置き換える）。
    fs::copy(
        &setup,
        packages.join(format!("{}-{version}-win-Setup.exe", options.pack_id)),
    )?;
    let mut archive = zip::ZipArchive::new(fs::File::open(&nupkg)?)?;
    let names = (0..archive.len())
        .map(|index| Ok(archive.by_index_raw(index)?.name()?.to_string()))
        .collect::<Result<Vec<_>>>()?;
    let pes = package_pe_names(&names)?;
    #[cfg(windows)]
    if options.sign {
        // アプリ（src/app_update/trust.rs）は適用前に、nupkg 内の全ての PE の署名者を同じ CN で照合する。
        // 配信前に同じ検査（加えて Certum の発行者・タイムスタンプ）を通し、通らない一式は配信しない。
        crate::windows_signing::verify(&setup)?;
        for (index, name) in pes.iter().enumerate() {
            let extracted = temp.join(format!("verify-{index}.bin"));
            {
                let mut entry = archive.by_name(name)?;
                let mut file = fs::File::create(&extracted)?;
                std::io::copy(&mut entry, &mut file)?;
            }
            let verified = crate::windows_signing::verify(&extracted);
            let _ = fs::remove_file(&extracted);
            verified.map_err(|error| format!("{name}: {error}"))?;
        }
        println!("nupkg内の{}個のPEの署名者を確認しました", pes.len());
    }
    #[cfg(not(windows))]
    let _ = pes;
    println!(
        "Setup: {} (SHA256 {})",
        setup.display(),
        sha256_hex(&fs::read(&setup)?)
    );
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct PublishOptions {
    packages: PathBuf,
    channel: String,
    pack_id: String,
    dry_run: bool,
}

impl PublishOptions {
    fn parse(args: &[String]) -> Result<Self> {
        let options = parse_options(
            args,
            &["--packages", "--channel", "--pack-id"],
            &["--dry-run"],
        )?;
        let value = |name: &str| options.get(name).cloned().flatten();
        let parsed = Self {
            packages: absolute(
                &value("--packages").ok_or("--packages（梱包の出力フォルダー）が必要です")?,
                "--packages",
            )?,
            channel: value("--channel").unwrap_or_else(|| "win".into()),
            pack_id: value("--pack-id").unwrap_or_else(|| PROD_PACK_ID.into()),
            dry_run: options.contains_key("--dry-run"),
        };
        if !parsed
            .channel
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            || parsed.channel.is_empty()
        {
            return Err("channelは小文字英数字とハイフンにしてください".into());
        }
        if !valid_pack_id(&parsed.pack_id) {
            return Err("packIdは英字で始まる英数字にしてください".into());
        }
        Ok(parsed)
    }
}

/// 公開する1ファイル。
#[derive(Debug, PartialEq, Eq)]
struct Upload {
    name: String,
    /// 版付きの成果物（新規作成のみ）。偽なら固定名（置き換え）。
    immutable: bool,
    content_type: &'static str,
    cache_control: &'static str,
}

/// 置く順。最新を指す一覧（`releases.<channel>.json`）を最後に置き、クライアントから未配信の nupkg が
/// 見える時間を作らない。
fn order(name: &str) -> u8 {
    if name.starts_with("releases.") && name.ends_with(".json") {
        3
    } else if name.starts_with("RELEASES") {
        2
    } else if name.starts_with("assets.") && name.ends_with(".json") {
        1
    } else {
        0
    }
}

/// フォルダーの中身から、置くファイルと順序を決める。必要なファイルが無い・名前が想定外なら失敗する。
fn plan_uploads(names: &[String], channel: &str, pack_id: &str) -> Result<Vec<Upload>> {
    let versioned = regex::Regex::new(r"\d+\.\d+\.\d+")?;
    let mut uploads = Vec::new();
    for name in names {
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
            || name.starts_with('.')
        {
            return Err(format!("公開できない名前です: {name}").into());
        }
        let immutable = name.ends_with(".nupkg") || versioned.is_match(name);
        let content_type = if name.ends_with(".json") {
            "application/json"
        } else if name.ends_with(".exe") {
            "application/vnd.microsoft.portable-executable"
        } else if name.starts_with("RELEASES") {
            "text/plain; charset=utf-8"
        } else {
            "application/octet-stream"
        };
        uploads.push(Upload {
            name: name.clone(),
            immutable,
            content_type,
            cache_control: if immutable {
                "public, max-age=31536000, immutable"
            } else {
                "no-cache, must-revalidate"
            },
        });
    }
    let manifest = format!("releases.{channel}.json");
    if !uploads.iter().any(|upload| upload.name == manifest) {
        return Err(format!("{manifest}がありません").into());
    }
    if !uploads.iter().any(|upload| {
        upload.name.starts_with(&format!("{pack_id}-")) && upload.name.ends_with("-full.nupkg")
    }) {
        return Err("full nupkgがありません".into());
    }
    uploads.sort_by(|a, b| (order(&a.name), &a.name).cmp(&(order(&b.name), &b.name)));
    Ok(uploads)
}

fn publish(options: &PublishOptions) -> Result {
    let mut names = Vec::new();
    for entry in fs::read_dir(&options.packages)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.is_dir() {
            continue;
        }
        if !metadata.is_file() {
            return Err("梱包の出力に通常ファイル以外があります".into());
        }
        names.push(
            entry
                .file_name()
                .to_str()
                .ok_or("ファイル名がUTF-8ではありません")?
                .to_string(),
        );
    }
    let uploads = plan_uploads(&names, &options.channel, &options.pack_id)?;
    if options.dry_run {
        for upload in &uploads {
            println!(
                "{:<10} {FEED_PREFIX}{}",
                if upload.immutable {
                    "immutable"
                } else {
                    "replace"
                },
                upload.name
            );
        }
        return Ok(());
    }
    // 公開バケット以外（非公開の CI 成果物バケットなど）へ誤って書かない。
    if env::var(format!("{UPDATE_ENV_PREFIX}BUCKET")).as_deref() != Ok(UPDATE_BUCKET) {
        return Err(format!("{UPDATE_ENV_PREFIX}BUCKETは{UPDATE_BUCKET}にしてください").into());
    }
    let store = Store::from_env_prefix(UPDATE_ENV_PREFIX)?;
    publish_uploads(&store, &options.packages, &uploads)?;
    println!("https://{UPDATE_BUCKET}.kagayoi.com/{FEED_PREFIX} へ公開しました");
    Ok(())
}

/// 計画どおりに置き、置いた内容を読み直して照合する。
fn publish_uploads(store: &Store, directory: &Path, uploads: &[Upload]) -> Result {
    for upload in uploads {
        let path = directory.join(&upload.name);
        let local = fs::read(&path)?;
        if local.len() > MAX_FILE {
            return Err(format!("{}が上限を超えています", upload.name).into());
        }
        let key = format!("{FEED_PREFIX}{}", upload.name);
        let mode = if upload.immutable {
            PutMode::CreateOnly
        } else {
            PutMode::Overwrite
        };
        let status =
            store.put_object(&key, &path, mode, upload.content_type, upload.cache_control)?;
        // 版付きの既存オブジェクトは上書きせず、内容が同じなら成功（再実行）とし、違えば失敗にする。
        let remote = store
            .get_object(&key, local.len())?
            .ok_or_else(|| format!("{key}を保存後に読み直せません"))?;
        if sha256_hex(&remote) != sha256_hex(&local) {
            return Err(if status == 412 {
                format!("{key}は既に別の内容で公開されています（上書きしません）")
            } else {
                format!("{key}の保存内容が一致しません")
            }
            .into());
        }
        println!(
            "  {} {key}",
            match (upload.immutable, status) {
                (true, 412) => "exists",
                (true, _) => "created",
                _ => "replaced",
            }
        );
    }
    Ok(())
}

/// 隔離E2E。テスト用 packId の 9.0.0・9.0.1 を未署名で梱包し、`tests/setup_velopack_e2e.rs` を起動する。
fn e2e(args: &[String]) -> Result {
    if !args.is_empty() {
        return Err("test-setup-velopack-e2eは引数を取りません（出力先はPLUMIVOX_SETUP_VELOPACK_E2E_OUTPUT_DIR）".into());
    }
    if !cfg!(windows) {
        return Err("Setup（Velopack）E2EはWindowsで実行してください".into());
    }
    require_vpk()?;
    // 出力先。未指定時は他の E2E と同じくユーザーの一時フォルダー配下の Codex/Plumivox/<乱数>。
    let work = match env::var_os("PLUMIVOX_SETUP_VELOPACK_E2E_OUTPUT_DIR").filter(|v| !v.is_empty())
    {
        Some(path) => absolute(
            path.to_str().ok_or("出力先がUTF-8ではありません")?,
            "PLUMIVOX_SETUP_VELOPACK_E2E_OUTPUT_DIR",
        )?,
        None => env::temp_dir().join("Codex").join("Plumivox").join(format!(
            "velopack-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        )),
    };
    if work.starts_with(root()) {
        return Err("E2Eの出力先はリポジトリの外にしてください".into());
    }
    if work.exists() {
        require_empty_plain_directory(&work)?;
    } else {
        fs::create_dir_all(&work)?;
    }
    let build = work.join("build");
    fs::create_dir(&build)?;
    let mut stub = None;
    for version in E2E_VERSIONS {
        // 版ごとに `--version` の表示を変えたテスト用ビルド（option_env! の変化で bin だけが再リンクされる）。
        let artifacts = cargo_build_package(
            "Cargo.toml",
            &[
                "build",
                "--locked",
                "--features",
                "update-feed-test,test-fixture",
                "--bin",
                "plumivox",
                "--bin",
                "plumivox-cli-stub",
            ],
            &["plumivox", "plumivox-cli-stub"],
            &[("PLUMIVOX_TEST_PACK_VERSION", version)],
        )?;
        let directory = build.join(version);
        fs::create_dir(&directory)?;
        fs::copy(
            artifacts.executable("plumivox")?,
            directory.join("plumivox.exe"),
        )?;
        if stub.is_none() {
            let copy = build.join("plumivox-cli-stub.exe");
            fs::copy(artifacts.executable("plumivox-cli-stub")?, &copy)?;
            stub = Some(copy);
        }
        let pack_work = work.join(format!("pack-{version}"));
        fs::create_dir(&pack_work)?;
        pack(&PackOptions {
            work: pack_work,
            sign: false,
            pack_id: TEST_PACK_ID.into(),
            version: Some(version.into()),
            binary: Some(directory.join("plumivox.exe")),
        })?;
    }
    let feed = |version: &str| work.join(format!("pack-{version}")).join("packages");
    let status = quiet(
        Command::new(crate::cargo())
            .current_dir(root())
            .args([
                "test",
                "--locked",
                "--features",
                "test-fixture,update-feed-test",
                "--test",
                "setup_velopack_e2e",
                "--",
                "--nocapture",
                "--test-threads=1",
            ])
            .env_remove("PLUMIVOX_TEST_PACK_VERSION")
            .env("PLUMIVOX_VELOPACK_E2E_OUTPUT_DIR", work.join("suite"))
            .env(
                "PLUMIVOX_VELOPACK_E2E_SETUP",
                feed(E2E_VERSIONS[0]).join(format!("{TEST_PACK_ID}-win-Setup.exe")),
            )
            .env("PLUMIVOX_VELOPACK_E2E_FEED_V1", feed(E2E_VERSIONS[0]))
            .env("PLUMIVOX_VELOPACK_E2E_FEED_V2", feed(E2E_VERSIONS[1]))
            .env(
                "PLUMIVOX_VELOPACK_E2E_STUB",
                stub.ok_or("CLIスタブがビルドされませんでした")?,
            ),
    )
    .status()?;
    println!("Setup（Velopack）E2Eの出力: {}", work.display());
    if !status.success() {
        return Err(format!("Setup（Velopack）E2Eが失敗しました ({status})").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn pack_options_refuse_signing_test_packages() {
        let work = env::temp_dir().join("velopack-work");
        let work_text = work.to_str().unwrap();
        let parsed = PackOptions::parse(&strings(&["--work", work_text, "--sign"])).unwrap();
        assert!(parsed.sign && parsed.pack_id == PROD_PACK_ID && parsed.version.is_none());
        // 署名はビルド済みの実行ファイル（CI の前段の step の成果物）とは併用できる。
        let binary = env::temp_dir().join("plumivox.exe");
        assert!(
            PackOptions::parse(&strings(&[
                "--work",
                work_text,
                "--sign",
                "--binary",
                binary.to_str().unwrap()
            ]))
            .is_ok()
        );
        for bad in [
            &["--work", work_text, "--sign", "--pack-id", TEST_PACK_ID][..],
            &["--work", work_text, "--sign", "--version", "9.0.0"],
            &["--work", work_text, "--pack-id", "1bad"],
            &["--work", work_text, "--version", "9.0"],
            &["--work", work_text, "--version", "09.0.0"],
            &["--work", "relative"],
            &["--sign"],
            &["--work", work_text, "--work", work_text],
            &["--work", work_text, "--unknown"],
        ] {
            assert!(PackOptions::parse(&strings(bad)).is_err(), "{bad:?}");
        }
        let test = PackOptions::parse(&strings(&[
            "--work",
            work_text,
            "--pack-id",
            TEST_PACK_ID,
            "--version",
            "9.0.1",
        ]))
        .unwrap();
        assert!(test.is_test_package());
    }

    #[test]
    fn package_version_is_read_without_cargo() {
        let manifest = "[package]\nname = \"plumivox\"\nversion = \"1.0.14\"\n\n[dependencies]\nfoo = { version = \"9.9.9\" }\n";
        assert_eq!(package_version(manifest).unwrap(), "1.0.14");
        assert!(package_version("[package]\nversion = \"1.0\"\n").is_err());
        assert!(package_version("[dependencies]\nversion = \"1.0.0\"\n").is_err());
        // 実際の Cargo.toml も読める。
        package_version(&fs::read_to_string(root().join("Cargo.toml")).unwrap()).unwrap();
    }

    #[test]
    fn nupkg_must_hold_one_main_executable_and_updater() {
        let base = strings(&[
            "PlumivoxApp.nuspec",
            "lib/app/plumivox.exe",
            "lib/app/Squirrel.exe",
            "lib/app/Plumivox_ExecutionStub.exe",
            "lib/app/sq.version",
        ]);
        assert_eq!(package_pe_names(&base).unwrap().len(), 3);
        assert!(package_pe_names(&base[..2]).is_err());
        let mut duplicated = base.clone();
        duplicated.push("LIB/APP/PLUMIVOX.EXE".into());
        assert!(package_pe_names(&duplicated).is_err());
        let mut ambiguous = base;
        ambiguous.push("lib/../x.exe".into());
        assert!(package_pe_names(&ambiguous).is_err());
        assert!(contains_bytes(
            b"xxPLUMIVOX_UPDATE_FEED_DIRyy",
            TEST_BUILD_MARKER
        ));
        assert!(!contains_bytes(b"PLUMIVOX_UPDATE", TEST_BUILD_MARKER));
    }

    #[test]
    fn publish_plan_puts_versioned_objects_first_and_the_manifest_last() {
        let names = strings(&[
            "releases.win.json",
            "RELEASES",
            "assets.win.json",
            "PlumivoxApp-win-Setup.exe",
            "PlumivoxApp-1.0.15-win-Setup.exe",
            "PlumivoxApp-1.0.15-full.nupkg",
        ]);
        let plan = plan_uploads(&names, "win", PROD_PACK_ID).unwrap();
        let order: Vec<_> = plan.iter().map(|upload| upload.name.as_str()).collect();
        assert_eq!(
            order,
            [
                "PlumivoxApp-1.0.15-full.nupkg",
                "PlumivoxApp-1.0.15-win-Setup.exe",
                "PlumivoxApp-win-Setup.exe",
                "assets.win.json",
                "RELEASES",
                "releases.win.json"
            ]
        );
        let immutable: Vec<_> = plan.iter().map(|upload| upload.immutable).collect();
        assert_eq!(immutable, [true, true, false, false, false, false]);
        assert_eq!(plan[5].content_type, "application/json");
        assert_eq!(plan[5].cache_control, "no-cache, must-revalidate");
        assert_eq!(plan[0].cache_control, "public, max-age=31536000, immutable");
        // 一覧・nupkg が無い、想定外の名前は拒否する。
        assert!(plan_uploads(&names[1..], "win", PROD_PACK_ID).is_err());
        assert!(plan_uploads(&strings(&["releases.win.json"]), "win", PROD_PACK_ID).is_err());
        let mut odd = names.clone();
        odd.push("../x".into());
        assert!(plan_uploads(&odd, "win", PROD_PACK_ID).is_err());
        let mut hidden = names;
        hidden.push(".secret".into());
        assert!(plan_uploads(&hidden, "win", PROD_PACK_ID).is_err());
    }

    fn fixture_directory(files: &[(&str, &[u8])]) -> PathBuf {
        let directory = env::temp_dir().join(format!(
            "velopack-publish-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        for (name, bytes) in files {
            fs::write(directory.join(name), bytes).unwrap();
        }
        directory
    }

    /// 実 curl と偽の S3 サーバーで、条件付き作成・置き換え・読み直し・順序を確かめる。
    #[test]
    fn publish_is_conditional_for_versioned_objects_and_verifies_bytes() {
        let nupkg: &[u8] = b"nupkg bytes";
        let manifest: &[u8] = b"{\"Assets\":[]}";
        let directory = fixture_directory(&[
            ("PlumivoxApp-1.0.15-full.nupkg", nupkg),
            ("releases.win.json", manifest),
        ]);
        let uploads = plan_uploads(
            &strings(&["PlumivoxApp-1.0.15-full.nupkg", "releases.win.json"]),
            "win",
            PROD_PACK_ID,
        )
        .unwrap();
        let (store, handle) = crate::r2::tests::server(vec![
            ("200 OK", vec![]),
            ("200 OK", nupkg.to_vec()),
            ("200 OK", vec![]),
            ("200 OK", manifest.to_vec()),
        ]);
        publish_uploads(&store, &directory, &uploads).unwrap();
        let requests = handle.join().unwrap();
        let text: Vec<String> = requests
            .iter()
            .map(|request| String::from_utf8_lossy(request).to_lowercase())
            .collect();
        assert!(text[0].starts_with("put /velopack/plumivoxapp-1.0.15-full.nupkg "));
        assert!(text[0].contains("if-none-match: *"));
        assert!(text[0].contains("cache-control: public, max-age=31536000, immutable"));
        assert!(text[1].starts_with("get /velopack/plumivoxapp-1.0.15-full.nupkg "));
        assert!(text[2].starts_with("put /velopack/releases.win.json "));
        assert!(!text[2].contains("if-none-match"));
        assert!(text[2].contains("content-type: application/json"));
        assert!(text[2].contains("cache-control: no-cache, must-revalidate"));
        assert!(requests[2].ends_with(manifest));

        // 既存（412）でも同じ内容なら成功、違えば失敗し、一覧は置かない。
        let (store, handle) = crate::r2::tests::server(vec![
            ("412 Precondition Failed", vec![]),
            ("200 OK", nupkg.to_vec()),
            ("200 OK", vec![]),
            ("200 OK", manifest.to_vec()),
        ]);
        publish_uploads(&store, &directory, &uploads).unwrap();
        handle.join().unwrap();
        let (store, handle) = crate::r2::tests::server(vec![
            ("412 Precondition Failed", vec![]),
            ("200 OK", b"other bytes".to_vec()),
        ]);
        let error = publish_uploads(&store, &directory, &uploads)
            .unwrap_err()
            .to_string();
        assert!(error.contains("上書きしません"), "{error}");
        let requests = handle.join().unwrap();
        assert_eq!(requests.len(), 2);
        // 置き換えのはずの固定名で 412 が返るのは異常として扱う。
        let (store, handle) = crate::r2::tests::server(vec![("412 Precondition Failed", vec![])]);
        assert!(publish_uploads(&store, &directory, &uploads[1..]).is_err());
        handle.join().unwrap();
        let _ = fs::remove_dir_all(directory);
    }
}
