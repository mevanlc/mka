use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn invoke(root: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mka"))
        .current_dir(root)
        .arg("--move")
        .args(arguments)
        .output()
        .unwrap()
}

fn success(output: Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

fn failure(output: Output) {
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("mka: "));
}

fn native_available(root: &Path) -> bool {
    let source = root.join("probe-source");
    let destination = root.join("probe-destination");
    fs::write(&source, b"probe").unwrap();
    match renamore::rename_exclusive(&source, &destination) {
        Ok(()) => {
            fs::remove_file(destination).unwrap();
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::Unsupported => {
            fs::remove_file(source).unwrap();
            false
        }
        Err(error) => panic!("cannot probe native move: {error}"),
    }
}

fn native_root() -> Option<tempfile::TempDir> {
    let root = tempfile::tempdir().unwrap();
    native_available(root.path()).then_some(root)
}

#[test]
fn platform_support_is_explicit_and_unsupported_moves_do_not_create_parents() {
    let root = tempfile::tempdir().unwrap();
    let available = native_available(root.path());
    if let Ok(expected) = std::env::var("MKA_EXPECT_NATIVE_MOVE") {
        assert_eq!(available, expected == "1");
    }
    fs::write(root.path().join("source"), b"keep").unwrap();
    let output = invoke(root.path(), &["source", "missing/deep/"]);
    if available {
        success(output);
        assert_eq!(
            fs::read(root.path().join("missing/deep/source")).unwrap(),
            b"keep"
        );
    } else {
        assert!(String::from_utf8_lossy(&output.stderr).contains("not supported"));
        failure(output);
        assert_eq!(fs::read(root.path().join("source")).unwrap(), b"keep");
        // On platforms lacking the API, support is checked before directory creation.
        if cfg!(any(
            target_os = "netbsd",
            target_os = "openbsd",
            target_os = "freebsd"
        )) {
            assert!(!root.path().join("missing").exists());
        }
    }
}

#[test]
fn moves_into_new_directories_and_renames_preserving_contents_and_metadata() {
    let Some(root) = native_root() else { return };
    let source = root.path().join("source");
    fs::write(&source, b"content").unwrap();
    let before = fs::metadata(&source).unwrap();
    success(invoke(root.path(), &["source", "new/deep/"]));
    let moved = root.path().join("new/deep/source");
    assert!(!source.exists());
    assert_eq!(fs::read(&moved).unwrap(), b"content");
    let after = fs::metadata(&moved).unwrap();
    assert_eq!(before.modified().unwrap(), after.modified().unwrap());
    assert_eq!(before.permissions(), after.permissions());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
    }
    success(invoke(root.path(), &["new/deep/source", "other/renamed"]));
    assert!(!moved.exists());
    assert_eq!(
        fs::read(root.path().join("other/renamed")).unwrap(),
        b"content"
    );
}

#[test]
fn multiple_sources_create_a_directory_even_without_a_separator() {
    let Some(root) = native_root() else { return };
    fs::write(root.path().join("a"), b"a").unwrap();
    fs::create_dir(root.path().join("tree")).unwrap();
    fs::write(root.path().join("tree/b"), b"b").unwrap();
    success(invoke(root.path(), &["a", "tree/", "new"]));
    assert_eq!(fs::read(root.path().join("new/a")).unwrap(), b"a");
    assert_eq!(fs::read(root.path().join("new/tree/b")).unwrap(), b"b");
    assert!(!root.path().join("a").exists());
    assert!(!root.path().join("tree").exists());
}

#[test]
fn directories_can_be_renamed_or_moved_into_existing_directories() {
    let Some(root) = native_root() else { return };
    fs::create_dir(root.path().join("tree")).unwrap();
    fs::write(root.path().join("tree/content"), b"tree").unwrap();
    success(invoke(root.path(), &["tree", "deep/renamed"]));
    fs::create_dir(root.path().join("existing")).unwrap();
    success(invoke(root.path(), &["deep/renamed", "existing"]));
    assert_eq!(
        fs::read(root.path().join("existing/renamed/content")).unwrap(),
        b"tree"
    );
}

#[test]
fn batch_preflight_rejects_missing_sources_duplicates_and_name_collisions() {
    for arguments in [
        vec!["a", "absent", "new/deep/"],
        vec!["a", "a", "new/deep/"],
        vec!["a", "./a", "new/deep/"],
        vec!["left/a", "right/a", "new/deep/"],
    ] {
        let Some(root) = native_root() else { return };
        for name in ["left", "right"] {
            fs::create_dir(root.path().join(name)).unwrap();
            fs::write(root.path().join(name).join("a"), name).unwrap();
        }
        fs::write(root.path().join("a"), b"a").unwrap();
        failure(invoke(root.path(), &arguments));
        assert!(!root.path().join("new").exists());
        assert_eq!(fs::read(root.path().join("a")).unwrap(), b"a");
        assert_eq!(fs::read(root.path().join("left/a")).unwrap(), b"left");
        assert_eq!(fs::read(root.path().join("right/a")).unwrap(), b"right");
    }
}

#[test]
fn occupied_destinations_never_replace_or_merge() {
    let Some(root) = native_root() else { return };
    fs::write(root.path().join("a"), b"a").unwrap();
    fs::write(root.path().join("b"), b"b").unwrap();
    fs::create_dir(root.path().join("dest")).unwrap();
    fs::write(root.path().join("dest/b"), b"keep").unwrap();
    failure(invoke(root.path(), &["a", "b", "dest/"]));
    assert_eq!(fs::read(root.path().join("a")).unwrap(), b"a");
    assert_eq!(fs::read(root.path().join("b")).unwrap(), b"b");
    assert_eq!(fs::read(root.path().join("dest/b")).unwrap(), b"keep");
    assert!(!root.path().join("dest/a").exists());
    failure(invoke(root.path(), &["a", "b"]));
    failure(invoke(root.path(), &["a", "a"]));
    failure(invoke(root.path(), &["a", "missing/../b"]));
    assert!(!root.path().join("missing").exists());

    fs::create_dir(root.path().join("tree")).unwrap();
    fs::write(root.path().join("tree/content"), b"tree").unwrap();
    fs::create_dir(root.path().join("dest/tree")).unwrap();
    failure(invoke(root.path(), &["tree", "dest/"]));
    assert_eq!(fs::read(root.path().join("tree/content")).unwrap(), b"tree");
    assert_eq!(
        fs::read_dir(root.path().join("dest/tree")).unwrap().count(),
        0
    );
}

#[cfg(windows)]
#[test]
fn windows_moves_symlinks_themselves() {
    let Some(root) = native_root() else { return };
    let source = root.path().join("link");
    match std::os::windows::fs::symlink_file("missing-target", &source) {
        Ok(()) => {}
        Err(error) if error.raw_os_error() == Some(1314) => return,
        Err(error) => panic!("cannot create source link: {error}"),
    }
    success(invoke(root.path(), &["link", "new/"]));
    assert!(fs::symlink_metadata(&source).is_err());
    assert_eq!(
        fs::read_link(root.path().join("new/link")).unwrap(),
        Path::new("missing-target")
    );
    fs::create_dir(root.path().join("real")).unwrap();
    std::os::windows::fs::symlink_dir("real", root.path().join("alias")).unwrap();
    success(invoke(root.path(), &["new/link", "alias/"]));
    success(invoke(root.path(), &["alias", "links/"]));
    assert_eq!(
        fs::read_link(root.path().join("links/alias")).unwrap(),
        Path::new("real")
    );
    assert!(root.path().join("real").is_dir());
}

#[test]
fn rejects_directory_containment_and_overlapping_sources_before_creation() {
    for arguments in [
        vec!["tree", "tree/new/deep/"],
        vec!["tree", "tree/newname"],
        vec!["tree", "tree/child", "new/"],
        vec!["tree/child", "tree", "new/"],
    ] {
        let Some(root) = native_root() else { return };
        fs::create_dir(root.path().join("tree")).unwrap();
        fs::write(root.path().join("tree/child"), b"keep").unwrap();
        failure(invoke(root.path(), &arguments));
        assert_eq!(fs::read(root.path().join("tree/child")).unwrap(), b"keep");
        assert_eq!(fs::read_dir(root.path().join("tree")).unwrap().count(), 1);
        assert!(!root.path().join("new").exists());
    }
}

#[test]
fn parents_and_existing_policies_apply_to_destinations_only() {
    let Some(root) = native_root() else { return };
    fs::write(root.path().join("source"), b"keep").unwrap();
    for destination in ["missing/", "missing/deep/name"] {
        failure(invoke(
            root.path(),
            &["--no-parents", "source", destination],
        ));
        assert!(!root.path().join("missing").exists());
    }
    fs::create_dir(root.path().join("existing")).unwrap();
    failure(invoke(
        root.path(),
        &["--no-exist-ok=dir", "source", "existing/new/"],
    ));
    assert!(!root.path().join("existing/new").exists());
    success(invoke(
        root.path(),
        &["--no-parents", "--no-exist-ok", "source", "existing/"],
    ));
    success(invoke(
        root.path(),
        &["--no-exist-ok=any", "existing/source", "new/deep/name"],
    ));
    assert_eq!(
        fs::read(root.path().join("new/deep/name")).unwrap(),
        b"keep"
    );
}

#[test]
fn move_paths_are_literal_and_handle_spaces_and_leading_hyphens() {
    let Some(root) = native_root() else { return };
    fs::write(root.path().join("name@@"), b"literal").unwrap();
    success(invoke(root.path(), &["name@@", "new dir@/"]));
    assert_eq!(
        fs::read(root.path().join("new dir@/name@@")).unwrap(),
        b"literal"
    );
    fs::write(root.path().join("--auto"), b"hyphen").unwrap();
    success(invoke(root.path(), &["--", "--auto", "new dir@/"]));
    assert_eq!(
        fs::read(root.path().join("new dir@/--auto")).unwrap(),
        b"hyphen"
    );
}

#[cfg(unix)]
#[test]
fn symlinks_move_as_entries_and_existing_link_destinations_are_preserved() {
    use std::os::unix::fs::symlink;
    let Some(root) = native_root() else { return };
    symlink("missing", root.path().join("link")).unwrap();
    success(invoke(root.path(), &["link", "new/"]));
    assert_eq!(
        fs::read_link(root.path().join("new/link")).unwrap(),
        Path::new("missing")
    );
    assert!(fs::symlink_metadata(root.path().join("link")).is_err());
    symlink("other-missing", root.path().join("occupied")).unwrap();
    failure(invoke(root.path(), &["new/link", "occupied"]));
    assert_eq!(
        fs::read_link(root.path().join("occupied")).unwrap(),
        Path::new("other-missing")
    );
    fs::create_dir(root.path().join("real")).unwrap();
    symlink("real", root.path().join("alias")).unwrap();
    success(invoke(root.path(), &["new/link", "alias/"]));
    assert_eq!(
        fs::read_link(root.path().join("real/link")).unwrap(),
        Path::new("missing")
    );
    failure(invoke(root.path(), &["alias/", "bad/"]));
    assert!(!root.path().join("bad").exists());
    success(invoke(root.path(), &["alias", "links/"]));
    assert_eq!(
        fs::read_link(root.path().join("links/alias")).unwrap(),
        Path::new("real")
    );
    assert!(root.path().join("real").is_dir());
}

#[cfg(unix)]
#[test]
fn preflight_resolves_aliases_for_containment_and_repeated_sources() {
    use std::os::unix::fs::symlink;
    for arguments in [
        vec!["tree", "alias/new/"],
        vec!["tree/child", "alias/child", "new/"],
    ] {
        let Some(root) = native_root() else { return };
        fs::create_dir(root.path().join("tree")).unwrap();
        fs::write(root.path().join("tree/child"), b"keep").unwrap();
        symlink("tree", root.path().join("alias")).unwrap();
        failure(invoke(root.path(), &arguments));
        assert!(!root.path().join("new").exists());
        assert!(!root.path().join("tree/new").exists());
        assert_eq!(fs::read(root.path().join("tree/child")).unwrap(), b"keep");
    }
}

#[cfg(unix)]
#[test]
fn moving_a_parent_alias_does_not_invalidate_another_source() {
    let Some(root) = native_root() else { return };
    fs::create_dir(root.path().join("real")).unwrap();
    fs::write(root.path().join("real/file"), b"keep").unwrap();
    std::os::unix::fs::symlink("real", root.path().join("alias")).unwrap();
    success(invoke(root.path(), &["alias", "alias/file", "new/"]));
    assert_eq!(fs::read(root.path().join("new/file")).unwrap(), b"keep");
    assert_eq!(
        fs::read_link(root.path().join("new/alias")).unwrap(),
        Path::new("real")
    );
}

#[cfg(all(unix, not(target_vendor = "apple")))]
#[test]
fn non_utf8_paths_are_preserved() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let Some(root) = native_root() else { return };
    let name = OsStr::from_bytes(b"file-\xff");
    fs::write(root.path().join(name), b"bytes").unwrap();
    success(
        Command::new(env!("CARGO_BIN_EXE_mka"))
            .current_dir(root.path())
            .args([OsStr::new("-m"), name, OsStr::new("new/")])
            .output()
            .unwrap(),
    );
    assert_eq!(
        fs::read(root.path().join("new").join(name)).unwrap(),
        b"bytes"
    );
}

#[cfg(unix)]
#[test]
fn special_sources_and_non_directory_parents_are_rejected_before_creation() {
    let Some(root) = native_root() else { return };
    let _socket = std::os::unix::net::UnixListener::bind(root.path().join("socket")).unwrap();
    fs::write(root.path().join("file"), b"keep").unwrap();
    failure(invoke(root.path(), &["file", "socket", "new/"]));
    assert!(!root.path().join("new").exists());
    failure(invoke(root.path(), &["file", "file/new/name"]));
    assert_eq!(fs::read(root.path().join("file")).unwrap(), b"keep");
}
