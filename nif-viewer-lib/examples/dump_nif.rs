//! Dump NIF scene mesh instance names + material info for heuristics research.
//! Usage: cargo run -p nif-viewer-lib --example dump_nif -- <path.nif>
fn main() {
    let path = std::env::args().nth(1).expect("usage: dump_nif <path.nif>");
    let bytes = std::fs::read(&path).expect("read nif");
    let scene = nif_viewer_lib::nif::parse_nif(&bytes).expect("parse nif");
    for m in &scene.meshes {
        println!(
            "name={:?} mat_path={:?} diffuse={:?} normal={:?}",
            m.name, m.material.mat_path, m.material.diffuse, m.material.normal
        );
    }
}
