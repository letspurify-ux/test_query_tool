use std::process::{Command, ExitStatus, Stdio};

/// Run `cargo test` and forward stdio to the current process.
pub fn run_cargo_test() -> std::io::Result<ExitStatus> {
    Command::new("cargo")
        .arg("test")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
}
