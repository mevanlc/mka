use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

fn invoke(root: &Path, arguments: &[&OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mka"))
        .current_dir(root)
        .args(arguments)
        .output()
        .expect("mka should run")
}

fn invoke_str(root: &Path, arguments: &[&str]) -> Output {
    let arguments: Vec<&OsStr> = arguments.iter().map(OsStr::new).collect();
    invoke(root, &arguments)
}

fn assert_success(output: Output) {
    assert!(
        output.status.success(),
        "status: {:?}\nstdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty(), "successful command wrote stdout");
    assert!(output.stderr.is_empty(), "successful command wrote stderr");
}

fn assert_runtime_error(output: Output) {
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("mka: "));
}

fn tempdir() -> TempDir {
    tempfile::tempdir().expect("temporary directory should be created")
}

#[test]
fn creates_nested_files_without_touching_existing_files() {
    let root = tempdir();
    let path = root.path().join("dir1/dir2/file");

    assert_success(invoke_str(root.path(), &["dir1/dir2/file"]));
    assert_eq!(fs::metadata(&path).unwrap().len(), 0);

    fs::write(&path, b"keep this content").unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    assert_success(invoke_str(root.path(), &["dir1/dir2/file"]));
    assert_eq!(fs::read(&path).unwrap(), b"keep this content");
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
}

#[test]
fn no_auto_still_upserts_parents_and_the_file() {
    let root = tempdir();
    assert_success(invoke_str(root.path(), &["--no-auto", "dir1/dir2/file"]));
    assert!(root.path().join("dir1/dir2/file").is_file());
}

#[test]
fn batches_mixed_files_and_directories() {
    let root = tempdir();
    assert_success(invoke_str(
        root.path(),
        &["one", "nested/dir/", "nested/dir/two"],
    ));
    assert!(root.path().join("one").is_file());
    assert!(root.path().join("nested/dir").is_dir());
    assert!(root.path().join("nested/dir/two").is_file());
}

#[test]
fn forced_types_bypass_auto_interpretation() {
    let root = tempdir();
    assert_success(invoke_str(root.path(), &["--type", "d", "directory"]));
    assert_success(invoke_str(root.path(), &["--type", "f", "literal@@"]));
    assert_success(invoke_str(root.path(), &["escaped@@"]));

    assert!(root.path().join("directory").is_dir());
    assert!(root.path().join("literal@@").is_file());
    assert!(root.path().join("escaped@").is_file());
}

#[test]
fn no_parents_fails_before_creating_a_missing_parent() {
    let root = tempdir();
    assert_runtime_error(invoke_str(root.path(), &["--no-parents", "missing/file"]));
    assert!(!root.path().join("missing").exists());

    fs::create_dir(root.path().join("existing")).unwrap();
    assert_success(invoke_str(root.path(), &["--no-parents", "existing/file"]));
}

#[test]
fn an_explicit_directory_can_supply_a_parent_with_no_parents() {
    let root = tempdir();
    assert_success(invoke_str(
        root.path(),
        &["--no-parents", "new/", "new/file"],
    ));
    assert!(root.path().join("new/file").is_file());
}

#[test]
fn wrong_leaf_types_are_errors() {
    let root = tempdir();
    fs::create_dir(root.path().join("directory")).unwrap();
    fs::write(root.path().join("file"), b"").unwrap();

    assert_runtime_error(invoke_str(root.path(), &["directory"]));
    assert_runtime_error(invoke_str(root.path(), &["--type", "d", "file"]));
}

#[test]
fn predictable_batch_conflicts_do_not_partially_mutate() {
    let root = tempdir();
    fs::create_dir(root.path().join("conflict")).unwrap();

    assert_runtime_error(invoke_str(root.path(), &["would-be-created", "conflict"]));
    assert!(!root.path().join("would-be-created").exists());

    assert_runtime_error(invoke_str(root.path(), &["path", "path/child"]));
    assert!(!root.path().join("path").exists());
}

#[test]
fn no_exist_ok_file_bare_and_explicit_forms_match() {
    for option in ["--no-exist-ok", "--no-exist-ok=file"] {
        let root = tempdir();
        fs::write(root.path().join("file"), b"unchanged").unwrap();
        assert_runtime_error(invoke_str(root.path(), &[option, "file"]));
        assert_eq!(fs::read(root.path().join("file")).unwrap(), b"unchanged");
    }
}

#[test]
fn no_exist_ok_dir_applies_to_named_parent_components() {
    let root = tempdir();
    fs::create_dir(root.path().join("existing")).unwrap();
    fs::write(root.path().join("existing-file"), b"unchanged").unwrap();

    assert_success(invoke_str(
        root.path(),
        &["--no-exist-ok=dir", "existing-file"],
    ));
    assert_eq!(
        fs::read(root.path().join("existing-file")).unwrap(),
        b"unchanged"
    );

    assert_runtime_error(invoke_str(
        root.path(),
        &["--no-exist-ok=dir", "existing/file"],
    ));
    assert!(!root.path().join("existing/file").exists());

    assert_success(invoke_str(
        root.path(),
        &["--no-exist-ok=dir", "fresh/file"],
    ));
}

#[test]
fn no_exist_ok_any_requires_all_named_components_to_be_new() {
    let root = tempdir();
    assert_success(invoke_str(
        root.path(),
        &["--no-exist-ok=any", "fresh/file"],
    ));

    assert_runtime_error(invoke_str(
        root.path(),
        &["--no-exist-ok=any", "fresh/other"],
    ));
    assert!(!root.path().join("fresh/other").exists());
}

#[cfg(any(unix, windows))]
fn symlinks_available(root: &Path) -> bool {
    #[cfg(unix)]
    {
        let _ = root;
        true
    }

    #[cfg(windows)]
    {
        let probe = root.join("privilege-probe");
        match std::os::windows::fs::symlink_file("target", &probe) {
            Ok(()) => {
                fs::remove_file(probe).unwrap();
                true
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::PermissionDenied
                    || error.raw_os_error() == Some(1314) =>
            {
                false
            }
            Err(error) => panic!("unexpected symbolic-link probe failure: {error}"),
        }
    }
}

#[test]
fn creates_links_in_either_auto_operand_order() {
    let root = tempdir();
    if !symlinks_available(root.path()) {
        return;
    }

    assert_success(invoke_str(root.path(), &["missing-target@", "links/first"]));
    assert_success(invoke_str(root.path(), &["links/second", "other-target@"]));
    assert_eq!(
        fs::read_link(root.path().join("links/first")).unwrap(),
        Path::new("missing-target")
    );
    assert_eq!(
        fs::read_link(root.path().join("links/second")).unwrap(),
        Path::new("other-target")
    );
}

#[test]
fn an_existing_link_is_idempotent_only_for_the_same_target() {
    let root = tempdir();
    if !symlinks_available(root.path()) {
        return;
    }

    assert_success(invoke_str(root.path(), &["target@", "link"]));
    assert_success(invoke_str(root.path(), &["target@", "link"]));
    assert_runtime_error(invoke_str(root.path(), &["different@", "link"]));
    assert_eq!(
        fs::read_link(root.path().join("link")).unwrap(),
        Path::new("target")
    );
}

#[test]
fn forced_link_mode_treats_at_signs_literally() {
    let root = tempdir();
    if !symlinks_available(root.path()) {
        return;
    }

    assert_success(invoke_str(
        root.path(),
        &["--type", "l", "target@", "link@"],
    ));
    assert_eq!(
        fs::read_link(root.path().join("link@")).unwrap(),
        Path::new("target@")
    );
}

#[test]
fn one_operand_link_uses_the_target_basename() {
    let root = tempdir();
    if !symlinks_available(root.path()) {
        return;
    }

    assert_success(invoke_str(root.path(), &["path/to/target@"]));
    assert_eq!(
        fs::read_link(root.path().join("target")).unwrap(),
        Path::new("path/to/target")
    );
}

#[test]
fn no_exist_ok_link_rejects_an_existing_same_target_link() {
    let root = tempdir();
    if !symlinks_available(root.path()) {
        return;
    }

    assert_success(invoke_str(root.path(), &["target@", "link"]));
    assert_runtime_error(invoke_str(
        root.path(),
        &["--no-exist-ok=link", "target@", "link"],
    ));
}

#[test]
fn no_exist_policy_does_not_apply_to_link_target_components() {
    let root = tempdir();
    if !symlinks_available(root.path()) {
        return;
    }

    fs::create_dir(root.path().join("existing-target-parent")).unwrap();
    assert_success(invoke_str(
        root.path(),
        &[
            "--no-exist-ok=any",
            "existing-target-parent/target@",
            "link",
        ],
    ));
}

#[test]
fn directory_symlinks_are_allowed_only_as_intermediate_parents() {
    let root = tempdir();
    if !symlinks_available(root.path()) {
        return;
    }

    fs::create_dir(root.path().join("real")).unwrap();
    create_directory_symlink(Path::new("real"), &root.path().join("alias"));

    assert_success(invoke_str(root.path(), &["alias/nested/file"]));
    assert!(root.path().join("real/nested/file").is_file());

    assert_runtime_error(invoke_str(root.path(), &["alias/"]));
    assert_runtime_error(invoke_str(
        root.path(),
        &["--no-exist-ok=link", "alias/other"],
    ));
}

#[cfg(unix)]
fn create_directory_symlink(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[cfg(windows)]
fn create_directory_symlink(target: &Path, link: &Path) {
    std::os::windows::fs::symlink_dir(target, link).unwrap();
}

#[cfg(windows)]
#[test]
fn windows_reports_symlink_privilege_errors_or_creates_the_link() {
    let root = tempdir();
    let output = invoke_str(root.path(), &["target@", "link"]);
    if symlinks_available(root.path()) {
        assert_success(output);
    } else {
        assert_runtime_error(output);
    }
}

#[cfg(windows)]
#[test]
fn windows_resolves_target_kind_relative_to_the_link_parent() {
    let root = tempdir();
    if !symlinks_available(root.path()) {
        return;
    }

    fs::create_dir(root.path().join("links")).unwrap();
    fs::create_dir(root.path().join("links/target-dir")).unwrap();
    assert_success(invoke_str(
        root.path(),
        &["--type", "l", "target-dir", "links/link"],
    ));
    assert!(
        fs::metadata(root.path().join("links/link"))
            .unwrap()
            .is_dir()
    );

    assert_success(invoke_str(
        root.path(),
        &["missing-dir/@", "links/dangling"],
    ));
    fs::create_dir(root.path().join("links/missing-dir")).unwrap();
    assert!(
        fs::metadata(root.path().join("links/dangling"))
            .unwrap()
            .is_dir()
    );
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn non_unicode_file_names_are_preserved() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let root = tempdir();
    let encoded = OsString::from_vec(b"non-utf8-\xff@@".to_vec());
    assert_success(invoke(root.path(), &[encoded.as_os_str()]));

    let expected = OsString::from_vec(b"non-utf8-\xff@".to_vec());
    assert!(root.path().join(expected).is_file());
}
