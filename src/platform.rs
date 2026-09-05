use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};

#[cfg(windows)]
use std::fs;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LinkKind {
    File,
    Directory,
}

#[cfg(unix)]
pub(crate) fn decode_terminal_ats(value: &OsStr) -> (OsString, bool) {
    use std::os::unix::ffi::OsStringExt;

    let bytes = value.as_encoded_bytes();
    let trailing = bytes.iter().rev().take_while(|&&byte| byte == b'@').count();
    let mut decoded = bytes[..bytes.len() - trailing].to_vec();
    decoded.extend(std::iter::repeat_n(b'@', trailing / 2));
    (OsString::from_vec(decoded), trailing % 2 == 1)
}

#[cfg(windows)]
pub(crate) fn decode_terminal_ats(value: &OsStr) -> (OsString, bool) {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    let units: Vec<u16> = value.encode_wide().collect();
    let trailing = units
        .iter()
        .rev()
        .take_while(|&&unit| unit == b'@' as u16)
        .count();
    let mut decoded = units[..units.len() - trailing].to_vec();
    decoded.extend(std::iter::repeat_n(b'@' as u16, trailing / 2));
    (OsString::from_wide(&decoded), trailing % 2 == 1)
}

#[cfg(unix)]
pub(crate) fn ends_in_separator(value: &OsStr) -> bool {
    value.as_encoded_bytes().last() == Some(&b'/')
}

#[cfg(windows)]
pub(crate) fn ends_in_separator(value: &OsStr) -> bool {
    use std::os::windows::ffi::OsStrExt;

    matches!(value.encode_wide().last(), Some(unit) if unit == b'/' as u16 || unit == b'\\' as u16)
}

#[cfg(unix)]
pub(crate) fn resolve_link_kind(
    _target: &Path,
    _link_name: &Path,
    directory_hint: bool,
) -> io::Result<LinkKind> {
    Ok(if directory_hint {
        LinkKind::Directory
    } else {
        LinkKind::File
    })
}

#[cfg(windows)]
pub(crate) fn resolve_link_kind(
    target: &Path,
    link_name: &Path,
    directory_hint: bool,
) -> io::Result<LinkKind> {
    let target_to_probe = target_probe_path(target, link_name);

    match fs::metadata(target_to_probe) {
        Ok(metadata) if metadata.is_dir() => Ok(LinkKind::Directory),
        Ok(_) => Ok(LinkKind::File),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(if directory_hint {
            LinkKind::Directory
        } else {
            LinkKind::File
        }),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
pub(crate) fn create_symlink(target: &Path, link_name: &Path, _kind: LinkKind) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link_name)
}

#[cfg(windows)]
pub(crate) fn create_symlink(target: &Path, link_name: &Path, kind: LinkKind) -> io::Result<()> {
    match kind {
        LinkKind::File => std::os::windows::fs::symlink_file(target, link_name),
        LinkKind::Directory => std::os::windows::fs::symlink_dir(target, link_name),
    }
}

pub(crate) fn target_probe_path(target: &Path, link_name: &Path) -> PathBuf {
    if target.is_absolute() {
        target.to_path_buf()
    } else {
        link_name
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .join(target)
    }
}

#[cfg(not(any(unix, windows)))]
compile_error!("mka supports Unix-family systems and Windows");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_at_pairs_are_unescaped() {
        let (decoded, marker) = decode_terminal_ats(OsStr::new("name@@@@"));
        assert_eq!(decoded, OsStr::new("name@@"));
        assert!(!marker);
    }

    #[test]
    fn odd_terminal_at_marks_a_target() {
        let (decoded, marker) = decode_terminal_ats(OsStr::new("target@@@"));
        assert_eq!(decoded, OsStr::new("target@"));
        assert!(marker);
    }

    #[test]
    fn internal_ats_are_literal() {
        let (decoded, marker) = decode_terminal_ats(OsStr::new("a@@b"));
        assert_eq!(decoded, OsStr::new("a@@b"));
        assert!(!marker);
    }
}
