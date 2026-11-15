//! Setup（Velopack）の隔離E2Eで、PATH の先頭に `claude.exe`・`codex.exe` として置く代役。
//!
//! 本物の Claude Code・Codex の設定（`~/.claude.json`・`~/.codex/config.toml`）に触れずに、
//! Plumivox の登録・解除の手順（`mcp get` → 必要なときだけ `mcp remove` → `mcp add`）を確かめるために使う。
//! - 名前（実行ファイル名の幹）で `claude` / `codex` を演じ分ける。
//! - 呼び出しの引数を `<PLUMIVOX_CLI_STUB_DIR>/calls.jsonl` に1行ずつ記録する。
//! - MCP の登録を `<dir>/<tool>-mcp.json` で模倣する（`mcp get/add/remove`）。
//! - `<dir>/faults.json` で遅延・失敗を注入する（例: `{"claude":{"add_delay_ms":5000,"fail":["remove"]}}`）。
//!   遅延は記録の後、登録の前に入れる（フックの3秒の打ち切りで止められた add は登録を残さない）。
//! - `PLUMIVOX_CLI_STUB_DIR` が無ければ何もせず失敗する（誤って本物の環境で動かないようにする）。
use serde_json::{Map, Value, json};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn read_json(path: &Path) -> Value {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_else(|| json!({}))
}

fn write_json(path: &Path, value: &Value) {
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    fs::rename(&temporary, path).unwrap();
}

fn record(dir: &Path, tool: &str, args: &[String]) {
    let line = json!({
        "tool": tool,
        "args": args,
        "pid": std::process::id(),
        "at_ms": SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64),
    });
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("calls.jsonl"))
        .unwrap();
    // 1行を1回の書き込みで足し、並行する呼び出しの行が混ざらないようにする。
    let mut bytes = serde_json::to_vec(&line).unwrap();
    bytes.push(b'\n');
    file.write_all(&bytes).unwrap();
}

/// `--scope user` の有無を取り除き、残りの位置引数を返す。claude は利用者単位の指定を必須にする。
fn split_scope(tool: &str, args: &[String]) -> Option<Vec<String>> {
    let mut rest = Vec::new();
    let mut scoped = false;
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--scope" {
            if args.get(index + 1).map(String::as_str) != Some("user") {
                return None;
            }
            scoped = true;
            index += 2;
            continue;
        }
        rest.push(args[index].clone());
        index += 1;
    }
    match tool {
        "claude" if !scoped => None,
        "codex" if scoped => None,
        _ => Some(rest),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(dir) = std::env::var_os("PLUMIVOX_CLI_STUB_DIR").map(PathBuf::from) else {
        eprintln!("PLUMIVOX_CLI_STUB_DIR is not set");
        std::process::exit(97);
    };
    let tool = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.file_stem().map(|s| s.to_string_lossy().to_lowercase()))
        .unwrap_or_default();
    if !matches!(tool.as_str(), "claude" | "codex") {
        eprintln!("unexpected stub name");
        std::process::exit(98);
    }
    record(&dir, &tool, &args);
    let faults = read_json(&dir.join("faults.json"))[&tool].clone();
    let registry_path = dir.join(format!("{tool}-mcp.json"));
    let mut registry = read_json(&registry_path);
    if !registry.is_object() {
        registry = json!({});
    }
    let fail = |operation: &str| {
        faults["fail"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item == operation))
    };
    let code = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["mcp", "get", name] => {
            if fail("get") {
                1
            } else if let Some(entry) = registry.get(*name) {
                let entry_args = entry["args"]
                    .as_array()
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .unwrap_or_default();
                println!(
                    "{name}:\n  Scope: User config\n  Type: stdio\n  Command: {}\n  Args: {entry_args}",
                    entry["command"].as_str().unwrap_or_default()
                );
                0
            } else {
                eprintln!("No MCP server found with name: {name}");
                1
            }
        }
        ["mcp", "add", ..] => {
            let Some(rest) = split_scope(&tool, &args[2..]) else {
                eprintln!("invalid scope");
                std::process::exit(2);
            };
            // <name> -- <command> <args...>
            if rest.len() < 3 || rest[1] != "--" {
                eprintln!("invalid add arguments");
                std::process::exit(2);
            }
            if let Some(delay) = faults["add_delay_ms"].as_u64() {
                std::thread::sleep(Duration::from_millis(delay));
            }
            if fail("add") {
                1
            } else if registry.get(&rest[0]).is_some() {
                eprintln!("MCP server {} already exists", rest[0]);
                1
            } else {
                registry.as_object_mut().unwrap().insert(
                    rest[0].clone(),
                    json!({"command": rest[2], "args": rest[3..]}),
                );
                write_json(&registry_path, &registry);
                0
            }
        }
        ["mcp", "remove", ..] => {
            let Some(rest) = split_scope(&tool, &args[2..]) else {
                eprintln!("invalid scope");
                std::process::exit(2);
            };
            if rest.len() != 1 || fail("remove") {
                1
            } else if registry
                .as_object_mut()
                .map(|map: &mut Map<String, Value>| map.remove(&rest[0]).is_some())
                .unwrap_or(false)
            {
                write_json(&registry_path, &registry);
                0
            } else {
                eprintln!("No MCP server found with name: {}", rest[0]);
                1
            }
        }
        _ => 2,
    };
    std::process::exit(code);
}
