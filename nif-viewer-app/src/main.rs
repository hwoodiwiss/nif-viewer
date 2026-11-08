fn main() {
    if std::env::var_os("RUST_LOG").is_none() && cfg!(debug_assertions) {
        env_logger::Builder::new()
            .filter_level(log::LevelFilter::Info)
            .init();
    } else {
        env_logger::init();
    }
    if let Err(e) = nif_viewer_lib::run() {
        eprintln!("nif-viewer: {:#}", e);
        std::process::exit(1);
    }
}
