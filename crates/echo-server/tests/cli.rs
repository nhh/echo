//! Die Kommandozeile des Programms.

#![allow(clippy::unwrap_used)]

use std::process::Command;

fn server() -> Command {
    Command::new(env!("CARGO_BIN_EXE_echo-server"))
}

#[test]
fn prints_its_version() {
    let output = server().arg("--version").output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        text.trim(),
        format!("echo-server {}", echo_server::SERVER_VERSION)
    );
}

#[test]
fn rejects_unclear_switches() {
    let output = server()
        .args(["--approval", "vielleicht"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let text = String::from_utf8(output.stderr).unwrap();
    assert!(text.contains("on oder off erwartet"), "{text}");
}
