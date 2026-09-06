use std::process::Command;

fn writ() -> Command {
    Command::new(env!("CARGO_BIN_EXE_writ"))
}

#[test]
fn version_prints_the_package_version() {
    let output = writ().arg("--version").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.trim(), format!("writ {}", env!("CARGO_PKG_VERSION")));
}

#[test]
fn help_documents_the_path_overrides() {
    let output = writ().arg("--help").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("--db"), "{stdout}");
    assert!(stdout.contains("--config"), "{stdout}");
}

#[test]
fn no_command_exits_two_and_says_so() {
    let output = writ().output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("no command given"), "{stderr}");
}
