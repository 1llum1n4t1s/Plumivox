use std::{path::PathBuf, time::Duration};

// 任意APIや疑似故障の注入は配布バイナリへ組み込まない。
#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
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
    let Ok(url) = url::Url::parse(&args[0]) else {
        std::process::exit(2)
    };
    if url.scheme() != "http"
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
    let mode = args.get(4).filter(|value| value.starts_with("--"));
    if let Some(mode) = mode {
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
}
