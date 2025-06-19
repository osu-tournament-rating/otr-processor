use std::sync::Once;

static INIT: Once = Once::new();

/// Initialize test environment with RUST_LOG=WARN
pub fn init_test_env() {
    INIT.call_once(|| {
        std::env::set_var("RUST_LOG", "warn");
        // Initialize env_logger with the WARN level
        let _ = env_logger::builder().filter_level(log::LevelFilter::Warn).try_init();
    });
}
