use std::process::Command;

fn run_mock(scenario: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mock_sidecar"))
        .arg(scenario)
        .output()
        .unwrap_or_else(|error| panic!("mock sidecar should run: {error}"))
}

#[test]
fn emits_a_success_url_to_stdout() {
    let output = run_mock("success-stdout");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(output.status.success());
    assert!(stdout.contains("https://mock-success.trycloudflare.com"));
}

#[test]
fn emits_a_success_url_to_stderr() {
    let output = run_mock("success-stderr");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success());
    assert!(stderr.contains("https://mock-stderr.trycloudflare.com"));
}

#[test]
fn can_split_a_url_across_stderr_writes() {
    let output = run_mock("split-stderr");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(output.status.success());
    assert!(stderr.contains("https://mock-split.trycloudflare.com"));
}

#[test]
fn exits_with_a_deterministic_failure_code() {
    let output = run_mock("exit-early");

    assert_eq!(output.status.code(), Some(23));
}
