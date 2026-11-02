use std::io::Read;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let success = match args.as_slice() {
        [arg, rest @ ..] if arg == "--setup" => match plumivox::setup::run(rest).await {
            Ok(()) => true,
            Err(error) => {
                eprintln!("{}: {}", error.code, error.message);
                false
            }
        },
        [] => plumivox::run_stdio(plumivox::Options::default())
            .await
            .is_ok(),
        [arg] if arg == "--stdio" => plumivox::run_stdio(plumivox::Options::default())
            .await
            .is_ok(),
        [arg] if arg == "--version" => {
            println!("plumivox {}", env!("CARGO_PKG_VERSION"));
            true
        }
        [arg] if arg == "--proxy-daemon" => {
            // 専用の子プロセスだけで設定し、Unix socketを生成時から本人専用にする。
            #[cfg(unix)]
            unsafe {
                libc::umask(0o077);
            }
            let mut bytes = Vec::new();
            let input = std::io::stdin().take(8193).read_to_end(&mut bytes);
            if input.is_err() || bytes.len() > 8192 {
                false
            } else if let Ok(bootstrap) = serde_json::from_slice(&bytes) {
                plumivox::proxy::run_daemon(bootstrap).await.is_ok()
            } else {
                false
            }
        }
        _ => {
            eprintln!("使い方: plumivox [--stdio | --version]");
            false
        }
    };
    if !success {
        std::process::exit(1);
    }
}
