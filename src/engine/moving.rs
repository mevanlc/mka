use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use super::Plan;
use crate::cli::{Cli, move_request};
use crate::error::{MkaError, Result};
use crate::platform::ends_in_separator;

pub(super) fn run(cli: Cli) -> Result<()> {
    MovePlan::preflight(&cli)?.execute()
}

#[derive(Debug)]
struct Move {
    source: PathBuf,
    destination: PathBuf,
    directory: bool,
}

#[derive(Debug)]
struct MovePlan {
    directories: Plan,
    moves: Vec<Move>,
}

impl MovePlan {
    fn preflight(cli: &Cli) -> Result<Self> {
        let request = move_request(cli)?;
        ensure_platform_support()?;
        // Keep the trailing separator as an intent hint, then remove it before
        // inspecting entries so a symlink leaf cannot be dereferenced by lstat.
        let directory_hint = ends_in_separator(request.destination.as_os_str());
        let destination: PathBuf = request.destination.components().collect();
        let into_directory = request.sources.len() > 1
            || directory_hint
            || match fs::metadata(&destination) {
                Ok(metadata) => metadata.is_dir(),
                Err(error) if error.kind() == io::ErrorKind::NotFound => false,
                Err(error) => return Err(MkaError::io("cannot inspect", &destination, error)),
            };

        let mut directories = Plan::new(cli.no_parents, cli.no_exist_ok);
        let mut pending = Vec::new();
        for operand in request.sources {
            let source: PathBuf = operand.components().collect();
            let name = leaf_name(&source)?;
            let metadata = fs::symlink_metadata(&source)
                .map_err(|error| MkaError::io("cannot inspect move source", &source, error))?;
            let kind = metadata.file_type();
            if !(kind.is_file() || kind.is_dir() || kind.is_symlink()) {
                return Err(MkaError::runtime(format!(
                    "move source {source:?} is not a regular file, directory, or symbolic link"
                )));
            }
            if ends_in_separator(operand.as_os_str()) && !kind.is_dir() {
                return Err(MkaError::runtime(format!(
                    "move source {operand:?} ends in a separator but is not a directory"
                )));
            }
            let target = if into_directory {
                destination.join(name)
            } else {
                leaf_name(&destination)?;
                destination.clone()
            };
            ensure_absent(&target)?;
            directories.add_parent_directories(&target)?;
            pending.push((source, target, metadata));
        }
        directories.preflight()?;

        let mut sources = HashSet::new();
        let mut destinations = HashSet::new();
        let mut moves = Vec::new();
        for (source, destination, metadata) in pending {
            // Resolve parent aliases, retaining symlink leaves themselves. Use
            // these resolved paths for execution, too: moving an alias in this
            // batch must not invalidate another operand that traversed it.
            let source = if metadata.file_type().is_symlink() {
                resolved_leaf(&source)?
            } else {
                fs::canonicalize(&source)
                    .map_err(|error| MkaError::io("cannot resolve move source", &source, error))?
            };
            let destination = resolved_leaf(&destination)?;
            // A missing intermediate component can hide an existing leaf in
            // spellings such as new/../occupied until new has been created.
            ensure_absent(&destination)?;
            if !sources.insert(source.clone()) {
                return Err(MkaError::runtime(format!(
                    "repeated move source {source:?}"
                )));
            }
            if !destinations.insert(destination.clone()) {
                return Err(MkaError::runtime(format!(
                    "multiple sources would move to {destination:?}"
                )));
            }
            check_filesystem(&metadata, &destination)?;
            moves.push(Move {
                source,
                destination,
                directory: metadata.is_dir(),
            });
        }
        for directory in moves.iter().filter(|item| item.directory) {
            for item in &moves {
                if item.source != directory.source && item.source.starts_with(&directory.source) {
                    return Err(MkaError::runtime(format!(
                        "overlapping move sources {:?} and {:?}",
                        directory.source, item.source
                    )));
                }
                if item.destination.starts_with(&directory.source) {
                    return Err(MkaError::runtime(format!(
                        "destination {:?} is inside move source {:?}",
                        item.destination, directory.source
                    )));
                }
            }
        }
        Ok(Self { directories, moves })
    }

    fn execute(self) -> Result<()> {
        self.execute_with(|source, destination| renamore::rename_exclusive(source, destination))
    }

    fn execute_with(self, mut rename: impl FnMut(&Path, &Path) -> io::Result<()>) -> Result<()> {
        self.directories.execute()?;
        for item in self.moves {
            rename(&item.source, &item.destination)
                .map_err(|error| move_error(&item.source, &item.destination, error))?;
        }
        Ok(())
    }
}

fn move_error(source: &Path, destination: &Path, error: io::Error) -> MkaError {
    let detail = match error.kind() {
        io::ErrorKind::Unsupported => {
            "no-replace moves are not supported on this platform or filesystem".to_owned()
        }
        io::ErrorKind::CrossesDevices => "cross-filesystem moves are not supported".to_owned(),
        _ => error.to_string(),
    };
    MkaError::runtime(format!(
        "cannot move {source:?} to {destination:?}: {detail}"
    ))
}

fn ensure_platform_support() -> Result<()> {
    if !cfg!(any(
        target_os = "linux",
        target_vendor = "apple",
        windows,
        target_os = "freebsd"
    )) {
        return Err(MkaError::runtime(
            "no-replace moves are not supported on this platform",
        ));
    }
    #[cfg(target_os = "freebsd")]
    {
        // Empty operands cannot name or mutate an entry. The fork resolves the
        // API before examining them, so this also detects older libc/kernels.
        if let Err(error) = renamore::rename_exclusive("", "")
            && error.kind() == io::ErrorKind::Unsupported
        {
            return Err(MkaError::runtime(
                "no-replace moves are not supported on this FreeBSD installation",
            ));
        }
    }
    Ok(())
}

fn leaf_name(path: &Path) -> Result<&std::ffi::OsStr> {
    path.file_name().ok_or_else(|| {
        MkaError::usage(format!(
            "move path {path:?} must name an entry, not a root, '.' or '..'"
        ))
    })
}

fn ensure_absent(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(MkaError::runtime(format!(
            "move destination {path:?} already exists"
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(MkaError::io("cannot inspect move destination", path, error)),
    }
}

fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn resolved_leaf(path: &Path) -> Result<PathBuf> {
    Ok(resolve_directory(parent(path))?.join(leaf_name(path)?))
}

// Resolve existing ancestors, projecting missing components without erasing
// symlink/.. semantics. Directory preflight has already rejected dangling links.
fn resolve_directory(path: &Path) -> Result<PathBuf> {
    match fs::canonicalize(path) {
        Ok(resolved) => Ok(resolved),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let component = path.components().next_back();
            if !matches!(component, Some(Component::Normal(_) | Component::ParentDir)) {
                return Err(MkaError::io("cannot resolve directory", path, error));
            }
            let mut resolved = resolve_directory(parent(path))?;
            match component {
                Some(Component::Normal(name)) => resolved.push(name),
                Some(Component::ParentDir) => {
                    resolved.pop();
                }
                _ => return Err(MkaError::io("cannot resolve directory", path, error)),
            }
            Ok(resolved)
        }
        Err(error) => Err(MkaError::io("cannot resolve directory", path, error)),
    }
}

fn check_filesystem(source: &fs::Metadata, destination: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let mut ancestor = parent(destination);
        loop {
            match fs::metadata(ancestor) {
                Ok(metadata) => {
                    if source.dev() != metadata.dev() {
                        return Err(MkaError::runtime(format!(
                            "cross-filesystem move to {destination:?} is not supported"
                        )));
                    }
                    break;
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    ancestor = parent(ancestor)
                }
                Err(error) => {
                    return Err(MkaError::io(
                        "cannot inspect destination filesystem",
                        ancestor,
                        error,
                    ));
                }
            }
        }
    }
    #[cfg(not(unix))]
    let _ = (source, destination);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn plan(root: &Path) -> Result<MovePlan> {
        let cli = Cli::try_parse_from([
            std::ffi::OsStr::new("mka"),
            std::ffi::OsStr::new("--move"),
            root.join("source").as_os_str(),
            root.join("destination").as_os_str(),
        ])
        .unwrap();
        MovePlan::preflight(&cli)
    }

    #[test]
    fn a_destination_created_after_preflight_is_never_replaced() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let destination = root.path().join("destination");
        fs::write(&source, b"source").unwrap();
        if ensure_platform_support().is_err() {
            return;
        }
        let plan = plan(root.path()).unwrap();
        fs::write(&destination, b"competitor").unwrap();
        assert!(plan.execute().is_err());
        assert_eq!(fs::read(source).unwrap(), b"source");
        assert_eq!(fs::read(destination).unwrap(), b"competitor");
    }

    #[test]
    fn cross_device_and_unsupported_errors_never_trigger_a_copy_fallback() {
        if ensure_platform_support().is_err() {
            return;
        }
        for kind in [io::ErrorKind::CrossesDevices, io::ErrorKind::Unsupported] {
            let root = tempfile::tempdir().unwrap();
            fs::write(root.path().join("source"), b"keep").unwrap();
            let error = plan(root.path())
                .unwrap()
                .execute_with(|_, _| Err(kind.into()))
                .unwrap_err();
            let MkaError::Runtime(message) = error else {
                panic!("expected a runtime error")
            };
            assert!(message.contains("not supported"));
            assert_eq!(fs::read(root.path().join("source")).unwrap(), b"keep");
            assert!(!root.path().join("destination").exists());
        }
    }
}
