fn main() {
    if let Err(message) = plumivox_vps_installer::run(std::env::args().skip(1)) {
        eprintln!("エラー: {message}");
        std::process::exit(1);
    }
}
