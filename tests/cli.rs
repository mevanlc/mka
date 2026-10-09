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
    assert!(stdout.contains("mka --completion <SHELL>"));
    assert!(stdout.contains("-m, --move"));
    assert!(stdout.contains("mka --move [OPTIONS] <SOURCE>... <DEST>"));
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
        vec!["--move", "one"],
        vec!["--move", "--auto", "one", "two"],
        vec!["--move", "--no-auto", "one", "two"],
        vec!["--move", "--type", "d", "one", "two"],
        vec!["--move", "", "two"],
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

#[test]
fn completions_are_output_only_for_every_supported_shell() {
    for (shell, marker) in [
        ("bash", "_mka()"),
        ("elvish", "edit:completion:arg-completer[mka]"),
        ("fish", "complete -c mka"),
        (
            "powershell",
            "Register-ArgumentCompleter -Native -CommandName 'mka'",
        ),
        ("zsh", "#compdef mka"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_mka"))
            .current_dir(root.path())
            .args(["--completion", shell])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{shell}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains(marker), "{shell}: {stdout}");
        assert!(stdout.contains("completion"), "{shell}: {stdout}");
        assert!(stdout.contains("no-parents"), "{shell}: {stdout}");
        assert!(stdout.contains("move"), "{shell}: {stdout}");
        assert!(output.stderr.is_empty());
        assert_eq!(root.path().read_dir().unwrap().count(), 0);
    }
}

#[test]
fn invalid_completion_requests_do_not_create_paths() {
    for arguments in [
        vec!["--completion"],
        vec!["--completion", "invalid-shell"],
        vec!["--completion", "zsh", "nested/file"],
        vec!["--completion", "zsh", "--no-parents"],
        vec!["--type", "d", "--completion", "bash", "directory"],
        vec!["--move", "--completion", "bash"],
    ] {
        let root = tempfile::tempdir().unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_mka"))
            .current_dir(root.path())
            .args(&arguments)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).starts_with("error:"));
        assert_eq!(root.path().read_dir().unwrap().count(), 0);
    }
}

#[cfg(unix)]
#[test]
fn completion_exits_quietly_when_stdout_is_closed() {
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;
    use std::process::Stdio;

    let (writer, reader) = UnixStream::pair().unwrap();
    drop(reader);
    let output = Command::new(env!("CARGO_BIN_EXE_mka"))
        .args(["--completion", "zsh"])
        .stdout(Stdio::from(OwnedFd::from(writer)))
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
}
