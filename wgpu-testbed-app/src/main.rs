fn main() {
    #[cfg(debug_assertions)]
    enable_info_logging();
    wgpu_testbed_lib::run();
}

fn enable_info_logging() {
    unsafe {
        std::env::set_var("RUST_LOG", "info");
    }
}
