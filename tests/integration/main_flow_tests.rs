use serial_test::serial;
use std::process::Command;

/// Test that the application exits with error code when database connection fails
#[test]
#[serial]
fn test_application_exits_on_connection_failure() {
    // Build the processor binary
    let build_output = Command::new("cargo")
        .args(&["build", "--bin", "otr-processor"])
        .output()
        .expect("Failed to execute cargo build");

    if !build_output.status.success() {
        panic!(
            "Failed to build processor: {}\n{}",
            String::from_utf8_lossy(&build_output.stdout),
            String::from_utf8_lossy(&build_output.stderr)
        );
    }

    // Determine the correct binary path based on profile
    let binary_path = if cfg!(debug_assertions) {
        "target/debug/otr-processor"
    } else {
        "target/release/otr-processor"
    };

    // Run with invalid connection string
    let output = Command::new(binary_path)
        .env(
            "CONNECTION_STRING",
            "host=invalid_host port=5432 user=postgres password=wrong dbname=nonexistent"
        )
        .env("RUST_LOG", "error")
        .output()
        .expect("Failed to execute processor");

    // Should exit with error code
    assert!(!output.status.success(), "Process should fail with invalid connection");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Failed to connect to database") || stderr.contains("connection error"),
        "Should log connection error"
    );
    assert!(
        stderr.contains("Application cannot start without a valid database connection"),
        "Should log clear message about needing database connection"
    );
}

/// Test that the application handles missing CONNECTION_STRING environment variable
#[test]
#[serial]
fn test_application_exits_on_missing_connection_string() {
    // Build the processor binary
    let build_output = Command::new("cargo")
        .args(&["build", "--bin", "otr-processor"])
        .output()
        .expect("Failed to execute cargo build");

    if !build_output.status.success() {
        panic!(
            "Failed to build processor: {}\n{}",
            String::from_utf8_lossy(&build_output.stdout),
            String::from_utf8_lossy(&build_output.stderr)
        );
    }

    // Determine the correct binary path based on profile
    let binary_path = if cfg!(debug_assertions) {
        std::env::current_dir().unwrap().join("target/debug/otr-processor")
    } else {
        std::env::current_dir().unwrap().join("target/release/otr-processor")
    };

    // Create a temporary directory without .env file
    let temp_dir = std::env::temp_dir().join("otr_processor_test");
    std::fs::create_dir_all(&temp_dir).ok();

    // Run without CONNECTION_STRING and from a directory without .env
    let output = Command::new(&binary_path)
        .current_dir(&temp_dir)
        .env_clear() // Clear all environment variables
        .env("RUST_LOG", "error")
        .env("PATH", std::env::var("PATH").unwrap_or_default()) // Keep PATH for system
        .output()
        .expect("Failed to execute processor");

    // Clean up
    std::fs::remove_dir_all(&temp_dir).ok();

    // Should exit with error code
    assert!(
        !output.status.success(),
        "Process should fail without CONNECTION_STRING"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("CONNECTION_STRING environment variable must be set"),
        "Should report missing CONNECTION_STRING. Got: {}",
        stderr
    );
}
