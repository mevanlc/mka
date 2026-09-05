use std::path::{Path, PathBuf};

use clap::{Parser, ValueEnum};

use crate::error::{MkaError, Result};
use crate::platform::{decode_terminal_ats, ends_in_separator};

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum ForcedType {
    #[value(name = "f")]
    File,
    #[value(name = "d")]
    Directory,
    #[value(name = "l")]
    Link,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum ExistingKind {
    File,
    Dir,
    Link,
    Any,
}

impl ExistingKind {
    pub(crate) fn rejects(self, actual: ExistingKind) -> bool {
        self == Self::Any || self == actual
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "mka",
    version,
    about = "mk anything",
    after_help = "Automatic forms:\n  mka FILE...\n  mka DIR/...\n  mka TARGET@ [LINK_NAME]\n  mka [LINK_NAME] TARGET@"
)]
pub(crate) struct Cli {
    /// Use suffix-based type inference (the default)
    #[arg(short = 'a', long, conflicts_with_all = ["no_auto", "forced_type"])]
    pub(crate) auto: bool,

    /// Treat every operand literally as a regular-file path
    #[arg(long, conflicts_with = "auto")]
    pub(crate) no_auto: bool,

    /// Force each leaf type: f (file), d (directory), or l (symbolic link)
    #[arg(
        short = 't',
        long = "type",
        value_name = "TYPE",
        conflicts_with = "auto"
    )]
    pub(crate) forced_type: Option<ForcedType>,

    /// Require parent directories to exist instead of creating them
    #[arg(long)]
    pub(crate) no_parents: bool,

    /// Reject pre-existing FSEs of TYPE; without =TYPE, rejects files
    #[arg(
        long,
        value_name = "TYPE",
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "file"
    )]
    pub(crate) no_exist_ok: Option<ExistingKind>,

    /// Paths to create
    #[arg(value_name = "PATH", required = true, allow_hyphen_values = true)]
    pub(crate) paths: Vec<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Request {
    File(PathBuf),
    Directory(PathBuf),
    Link {
        target: PathBuf,
        link_name: PathBuf,
        directory_hint: bool,
    },
}

pub(crate) fn requests(cli: &Cli) -> Result<Vec<Request>> {
    if cli.no_auto {
        if matches!(cli.forced_type, Some(kind) if kind != ForcedType::File) {
            return Err(MkaError::usage(
                "--no-auto is equivalent to --type f and cannot be combined with --type d or l",
            ));
        }
        return forced_requests(ForcedType::File, &cli.paths);
    }

    match cli.forced_type {
        Some(kind) => forced_requests(kind, &cli.paths),
        None => automatic_requests(&cli.paths),
    }
}

fn forced_requests(kind: ForcedType, paths: &[PathBuf]) -> Result<Vec<Request>> {
    match kind {
        ForcedType::File => paths
            .iter()
            .map(|path| {
                validate_nonempty(path, "file path")?;
                if ends_in_separator(path.as_os_str()) {
                    return Err(MkaError::usage(format!(
                        "file path {path:?} ends in a path separator"
                    )));
                }
                Ok(Request::File(path.clone()))
            })
            .collect(),
        ForcedType::Directory => paths
            .iter()
            .map(|path| {
                validate_nonempty(path, "directory path")?;
                Ok(Request::Directory(normalize_directory_path(path)))
            })
            .collect(),
        ForcedType::Link => forced_link_request(paths),
    }
}

fn forced_link_request(paths: &[PathBuf]) -> Result<Vec<Request>> {
    if !(1..=2).contains(&paths.len()) {
        return Err(MkaError::usage(
            "--type l requires TARGET and an optional LINK_NAME",
        ));
    }

    let target = paths[0].clone();
    validate_nonempty(&target, "symbolic-link target")?;
    let link_name = match paths.get(1) {
        Some(link_name) => link_name.clone(),
        None => implicit_link_name(&target)?,
    };
    validate_link_name(&link_name)?;

    Ok(vec![Request::Link {
        directory_hint: ends_in_separator(target.as_os_str()),
        target,
        link_name,
    }])
}

fn automatic_requests(paths: &[PathBuf]) -> Result<Vec<Request>> {
    let decoded: Vec<DecodedOperand> = paths
        .iter()
        .map(|path| {
            let (value, target_marker) = decode_terminal_ats(path.as_os_str());
            let value = PathBuf::from(value);
            validate_nonempty(&value, "path")?;
            Ok(DecodedOperand {
                value,
                target_marker,
            })
        })
        .collect::<Result<_>>()?;

    let targets: Vec<usize> = decoded
        .iter()
        .enumerate()
        .filter_map(|(index, operand)| operand.target_marker.then_some(index))
        .collect();

    if targets.is_empty() {
        return Ok(decoded
            .into_iter()
            .map(|operand| {
                if ends_in_separator(operand.value.as_os_str()) {
                    Request::Directory(normalize_directory_path(&operand.value))
                } else {
                    Request::File(operand.value)
                }
            })
            .collect());
    }

    if targets.len() != 1 || decoded.len() > 2 {
        return Err(MkaError::usage(
            "automatic symbolic-link creation requires exactly one TARGET@ and at most one LINK_NAME",
        ));
    }

    let target_index = targets[0];
    let target = decoded[target_index].value.clone();
    let link_name = if decoded.len() == 1 {
        implicit_link_name(&target)?
    } else {
        decoded[1 - target_index].value.clone()
    };
    validate_link_name(&link_name)?;

    Ok(vec![Request::Link {
        directory_hint: ends_in_separator(target.as_os_str()),
        target,
        link_name,
    }])
}

#[derive(Debug)]
struct DecodedOperand {
    value: PathBuf,
    target_marker: bool,
}

fn implicit_link_name(target: &Path) -> Result<PathBuf> {
    target
        .file_name()
        .map(PathBuf::from)
        .ok_or_else(|| MkaError::usage(format!("cannot derive a link name from target {target:?}")))
}

fn validate_nonempty(path: &Path, label: &str) -> Result<()> {
    if path.as_os_str().is_empty() {
        Err(MkaError::usage(format!("{label} must not be empty")))
    } else {
        Ok(())
    }
}

fn validate_link_name(path: &Path) -> Result<()> {
    validate_nonempty(path, "symbolic-link name")?;
    if ends_in_separator(path.as_os_str()) {
        return Err(MkaError::usage(format!(
            "symbolic-link name {path:?} must name an exact leaf"
        )));
    }
    Ok(())
}

fn normalize_directory_path(path: &Path) -> PathBuf {
    path.components().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli(arguments: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("mka").chain(arguments.iter().copied())).unwrap()
    }

    #[test]
    fn auto_batches_files_and_directories() {
        let parsed = cli(&["file", "nested/dir/"]);
        assert_eq!(
            requests(&parsed).unwrap(),
            vec![
                Request::File(PathBuf::from("file")),
                Request::Directory(PathBuf::from("nested/dir/")),
            ]
        );
    }

    #[test]
    fn auto_link_target_can_come_first_or_last() {
        for arguments in [["target@", "link"], ["link", "target@"]] {
            assert_eq!(
                requests(&cli(&arguments)).unwrap(),
                vec![Request::Link {
                    target: PathBuf::from("target"),
                    link_name: PathBuf::from("link"),
                    directory_hint: false,
                }]
            );
        }
    }

    #[test]
    fn escaped_link_name_can_end_in_at() {
        assert_eq!(
            requests(&cli(&["target@", "link@@"])).unwrap(),
            vec![Request::Link {
                target: PathBuf::from("target"),
                link_name: PathBuf::from("link@"),
                directory_hint: false,
            }]
        );
    }

    #[test]
    fn implicit_link_name_uses_target_basename() {
        assert_eq!(
            requests(&cli(&["path/to/target@"])).unwrap(),
            vec![Request::Link {
                target: PathBuf::from("path/to/target"),
                link_name: PathBuf::from("target"),
                directory_hint: false,
            }]
        );
    }

    #[test]
    fn no_auto_is_literal_file_mode() {
        assert_eq!(
            requests(&cli(&["--no-auto", "path@"])).unwrap(),
            vec![Request::File(PathBuf::from("path@"))]
        );
    }

    #[test]
    fn forced_link_uses_ln_order_and_literal_ats() {
        assert_eq!(
            requests(&cli(&["--type", "l", "target@", "link@"])).unwrap(),
            vec![Request::Link {
                target: PathBuf::from("target@"),
                link_name: PathBuf::from("link@"),
                directory_hint: false,
            }]
        );
    }

    #[test]
    fn bare_no_exist_ok_does_not_consume_the_path() {
        let parsed = cli(&["--no-exist-ok", "file"]);
        assert_eq!(parsed.no_exist_ok, Some(ExistingKind::File));
        assert_eq!(parsed.paths, [PathBuf::from("file")]);
    }

    #[test]
    fn no_auto_rejects_non_file_forced_types() {
        let error = requests(&cli(&["--no-auto", "--type", "d", "path"])).unwrap_err();
        assert!(matches!(error, MkaError::Usage(_)));
    }

    #[test]
    fn auto_rejects_ambiguous_link_batches() {
        let error = requests(&cli(&["target@", "link", "extra"])).unwrap_err();
        assert!(matches!(error, MkaError::Usage(_)));
    }
}
