use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::cli::{Cli, ExistingKind, Request, requests};
use crate::error::{MkaError, Result};
use crate::platform::{self, LinkKind};

pub(crate) fn run(cli: Cli) -> Result<()> {
    let requests = requests(&cli)?;
    let mut plan = Plan::new(cli.no_parents, cli.no_exist_ok);
    for request in requests {
        plan.add_request(request)?;
    }
    plan.preflight()?;
    plan.execute()
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LinkSpec {
    target: PathBuf,
    directory_hint: bool,
    kind: Option<LinkKind>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Expected {
    Directory { explicit: bool },
    File,
    Link(LinkSpec),
}

#[derive(Clone, Debug)]
struct Node {
    path: PathBuf,
    expected: Expected,
    policy_relevant: bool,
    order: usize,
    missing: bool,
}

#[derive(Debug)]
struct Plan {
    nodes: Vec<Node>,
    by_path: HashMap<PathBuf, usize>,
    no_parents: bool,
    no_exist_ok: Option<ExistingKind>,
}

impl Plan {
    fn new(no_parents: bool, no_exist_ok: Option<ExistingKind>) -> Self {
        Self {
            nodes: Vec::new(),
            by_path: HashMap::new(),
            no_parents,
            no_exist_ok,
        }
    }

    fn add_request(&mut self, request: Request) -> Result<()> {
        match request {
            Request::File(path) => {
                self.add_parent_directories(&path)?;
                self.add_node(path, Expected::File)
            }
            Request::Directory(path) => {
                self.add_parent_directories(&path)?;
                self.add_node(path, Expected::Directory { explicit: true })
            }
            Request::Link {
                target,
                link_name,
                directory_hint,
            } => {
                self.add_parent_directories(&link_name)?;
                self.add_node(
                    link_name,
                    Expected::Link(LinkSpec {
                        target,
                        directory_hint,
                        kind: None,
                    }),
                )
            }
        }
    }

    fn add_parent_directories(&mut self, path: &Path) -> Result<()> {
        let mut parents: Vec<PathBuf> = path
            .ancestors()
            .skip(1)
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .collect();
        parents.reverse();

        for parent in parents {
            self.add_node(parent, Expected::Directory { explicit: false })?;
        }
        Ok(())
    }

    fn add_node(&mut self, path: PathBuf, expected: Expected) -> Result<()> {
        if let Some(&index) = self.by_path.get(&path) {
            let existing = &mut self.nodes[index].expected;
            return merge_expectations(&path, existing, expected);
        }

        let order = self.nodes.len();
        let policy_relevant = matches!(path.components().next_back(), Some(Component::Normal(_)));
        self.by_path.insert(path.clone(), order);
        self.nodes.push(Node {
            path,
            expected,
            policy_relevant,
            order,
            missing: false,
        });
        Ok(())
    }

    fn preflight(&mut self) -> Result<()> {
        for node in &mut self.nodes {
            match fs::symlink_metadata(&node.path) {
                Ok(metadata) => validate_existing(node, &metadata, self.no_exist_ok)?,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    if self.no_parents
                        && matches!(node.expected, Expected::Directory { explicit: false })
                    {
                        return Err(MkaError::runtime(format!(
                            "parent directory {:?} does not exist (--no-parents)",
                            node.path
                        )));
                    }

                    if let Expected::Link(spec) = &mut node.expected {
                        spec.kind = Some(
                            platform::resolve_link_kind(
                                &spec.target,
                                &node.path,
                                spec.directory_hint,
                            )
                            .map_err(|error| {
                                let probe = platform::target_probe_path(&spec.target, &node.path);
                                MkaError::io("cannot inspect symbolic-link target", &probe, error)
                            })?,
                        );
                    }
                    node.missing = true;
                }
                Err(error) => {
                    return Err(MkaError::io("cannot inspect", &node.path, error));
                }
            }
        }
        Ok(())
    }

    fn execute(mut self) -> Result<()> {
        let mut directories: Vec<Node> = self
            .nodes
            .iter()
            .filter(|node| node.missing && matches!(node.expected, Expected::Directory { .. }))
            .cloned()
            .collect();
        directories.sort_by_key(|node| (node.path.components().count(), node.order));

        for node in &directories {
            match fs::create_dir(&node.path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    revalidate_after_race(node, self.no_exist_ok)?;
                }
                Err(error) => {
                    return Err(MkaError::io("cannot create directory", &node.path, error));
                }
            }
        }

        self.nodes.sort_by_key(|node| node.order);
        for node in &self.nodes {
            if !node.missing || matches!(node.expected, Expected::Directory { .. }) {
                continue;
            }

            match &node.expected {
                Expected::File => create_file(node, self.no_exist_ok)?,
                Expected::Link(spec) => create_link(node, spec, self.no_exist_ok)?,
                Expected::Directory { .. } => unreachable!(),
            }
        }
        Ok(())
    }
}

fn merge_expectations(path: &Path, existing: &mut Expected, incoming: Expected) -> Result<()> {
    match (existing, incoming) {
        (
            Expected::Directory {
                explicit: existing_explicit,
            },
            Expected::Directory {
                explicit: incoming_explicit,
            },
        ) => {
            *existing_explicit |= incoming_explicit;
            Ok(())
        }
        (Expected::File, Expected::File) => Ok(()),
        (Expected::Link(existing), Expected::Link(incoming))
            if existing.target == incoming.target
                && existing.directory_hint == incoming.directory_hint =>
        {
            Ok(())
        }
        (existing, incoming) => Err(MkaError::runtime(format!(
            "conflicting requested types for {path:?}: {} and {}",
            expected_name(existing),
            expected_name(&incoming)
        ))),
    }
}

fn validate_existing(
    node: &Node,
    metadata: &fs::Metadata,
    no_exist_ok: Option<ExistingKind>,
) -> Result<()> {
    let actual = existing_kind(metadata);
    if node.policy_relevant && no_exist_ok.is_some_and(|policy| policy.rejects(actual)) {
        return Err(MkaError::runtime(format!(
            "{:?} already exists as {} (--no-exist-ok={})",
            node.path,
            existing_name(actual),
            policy_name(no_exist_ok.expect("checked above"))
        )));
    }

    match (&node.expected, actual) {
        (Expected::File, ExistingKind::File) => Ok(()),
        (Expected::Directory { .. }, ExistingKind::Dir) => Ok(()),
        (Expected::Directory { explicit: false }, ExistingKind::Link) => {
            match fs::metadata(&node.path) {
                Ok(target) if target.is_dir() => Ok(()),
                Ok(_) => Err(type_conflict(node, actual)),
                Err(error) => Err(MkaError::io(
                    "cannot traverse symbolic-link parent",
                    &node.path,
                    error,
                )),
            }
        }
        (Expected::Link(spec), ExistingKind::Link) => {
            let target = fs::read_link(&node.path)
                .map_err(|error| MkaError::io("cannot read symbolic link", &node.path, error))?;
            if target == spec.target {
                Ok(())
            } else {
                Err(MkaError::runtime(format!(
                    "symbolic link {:?} points to {target:?}, expected {:?}",
                    node.path, spec.target
                )))
            }
        }
        _ => Err(type_conflict(node, actual)),
    }
}

fn create_file(node: &Node, no_exist_ok: Option<ExistingKind>) -> Result<()> {
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&node.path)
    {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            revalidate_after_race(node, no_exist_ok)
        }
        Err(error) => Err(MkaError::io("cannot create file", &node.path, error)),
    }
}

fn create_link(node: &Node, spec: &LinkSpec, no_exist_ok: Option<ExistingKind>) -> Result<()> {
    let kind = spec.kind.unwrap_or(if spec.directory_hint {
        LinkKind::Directory
    } else {
        LinkKind::File
    });
    match platform::create_symlink(&spec.target, &node.path, kind) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            revalidate_after_race(node, no_exist_ok)
        }
        Err(error) => Err(MkaError::io(
            "cannot create symbolic link",
            &node.path,
            error,
        )),
    }
}

fn revalidate_after_race(node: &Node, no_exist_ok: Option<ExistingKind>) -> Result<()> {
    let metadata = fs::symlink_metadata(&node.path)
        .map_err(|error| MkaError::io("cannot inspect after creation race", &node.path, error))?;
    validate_existing(node, &metadata, no_exist_ok)
}

fn existing_kind(metadata: &fs::Metadata) -> ExistingKind {
    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        ExistingKind::Link
    } else if file_type.is_file() {
        ExistingKind::File
    } else if file_type.is_dir() {
        ExistingKind::Dir
    } else {
        ExistingKind::Any
    }
}

fn type_conflict(node: &Node, actual: ExistingKind) -> MkaError {
    MkaError::runtime(format!(
        "{:?} is {}, expected {}",
        node.path,
        existing_name(actual),
        expected_name(&node.expected)
    ))
}

fn expected_name(expected: &Expected) -> &'static str {
    match expected {
        Expected::Directory { .. } => "a directory",
        Expected::File => "a regular file",
        Expected::Link(_) => "a symbolic link",
    }
}

fn existing_name(kind: ExistingKind) -> &'static str {
    match kind {
        ExistingKind::File => "file",
        ExistingKind::Dir => "dir",
        ExistingKind::Link => "link",
        ExistingKind::Any => "another FSE type",
    }
}

fn policy_name(kind: ExistingKind) -> &'static str {
    match kind {
        ExistingKind::File => "file",
        ExistingKind::Dir => "dir",
        ExistingKind::Link => "link",
        ExistingKind::Any => "any",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_an_explicit_directory_with_a_parent_requirement() {
        let mut plan = Plan::new(false, None);
        plan.add_request(Request::File(PathBuf::from("dir/file")))
            .unwrap();
        plan.add_request(Request::Directory(PathBuf::from("dir")))
            .unwrap();

        let directory = &plan.nodes[*plan.by_path.get(Path::new("dir")).unwrap()];
        assert!(matches!(
            directory.expected,
            Expected::Directory { explicit: true }
        ));
    }

    #[test]
    fn rejects_a_leaf_that_another_request_needs_as_a_directory() {
        let mut plan = Plan::new(false, None);
        plan.add_request(Request::File(PathBuf::from("path")))
            .unwrap();
        let error = plan
            .add_request(Request::File(PathBuf::from("path/child")))
            .unwrap_err();
        assert!(matches!(error, MkaError::Runtime(_)));
    }
}
