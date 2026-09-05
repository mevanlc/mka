use std::process::{Command, Output};

fn invoke(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mka"))
        .args(arguments)
        .output()
        .expect("mka should run")
}

#[test]
fn help_describes_the_auto_and_forced_interfaces() {
    let output = invoke(&["--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("mk anything"));
    assert!(stdout.contains("--no-auto"));
    assert!(stdout.contains("-t, --type <TYPE>"));
    assert!(stdout.contains("--no-exist-ok[=<TYPE>]"));
    assert!(stdout.contains("mka TARGET@ [LINK_NAME]"));
    assert!(output.stderr.is_empty());
}

#[test]
fn grammar_errors_exit_two() {
    for arguments in [
        vec!["target@", "link", "extra"],
        vec!["--type", "l", "one", "two", "three"],
        vec!["--no-auto", "--type", "d", "path"],
        vec!["--type", "x", "path"],
    ] {
        let output = invoke(&arguments);
        assert_eq!(
            output.status.code(),
            Some(2),
            "arguments: {arguments:?}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).starts_with("error:"));
    }
}

#[test]
fn missing_paths_are_a_clap_error() {
    let output = invoke(&[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("error:"));
}
