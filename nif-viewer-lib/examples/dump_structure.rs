//! Dump NIF raw block structure JSON with debug logging enabled, to
//! diagnose block-size resync issues.
//! Usage: cargo run -p nif-viewer-lib --example dump_structure -- <path.nif>
fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug")).init();
    let path = std::env::args().nth(1).expect("usage: dump_structure <path.nif>");
    let bytes = std::fs::read(&path).expect("read nif");
    let structure = nif_viewer_lib::nif::nif_structure(&bytes).expect("parse nif");
    println!("{}", serde_json::to_string_pretty(&structure).unwrap());
}
