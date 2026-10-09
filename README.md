# mka

`mka` means “mk anything.” It creates regular files, directories, missing parent
directories, and symbolic links, and moves existing objects into new directories.
It supports macOS, Linux, Windows, FreeBSD, NetBSD, and OpenBSD.

Successful creation is silent. Creating an object that already has the requested
type is normally a no-op: existing regular files are not truncated or touched,
existing directories are accepted, and existing symbolic links are accepted only
when their stored target is exactly the requested target.

## Usage

Automatic type detection is enabled by default:

```console
# Empty files, with missing parents created automatically
mka file
mka dir1/dir2/file

# A directory (the trailing separator selects the type)
mka dir1/dir2/

# A symbolic link; @ marks the target, in either operand position
mka path/to/target@ path/to/link
mka path/to/link path/to/target@

# Like `ln -s path/to/target`: create `target` in the current directory
mka path/to/target@
```

Files and directories may be mixed in one invocation. A symbolic-link
invocation creates exactly one link and therefore accepts only the marked target
and an optional link name.

On Unix, `/` is the directory marker. On Windows, either `/` or `\` may be used.

### Literal and forced types

Use `-t`/`--type` when a suffix should not select the leaf type:

```console
mka --type f 'literal@'
mka --type d directory-without-a-trailing-separator
mka --type l 'literal-target@' 'literal-link@'
```

The accepted values are `f` (regular file), `d` (directory), and `l` (symbolic
link). Forced link mode follows `ln -s` operand order: `TARGET [LINK_NAME]`.
`--type l` does not interpret or unescape `@` characters.

`--no-auto` is equivalent to `--type f`, including automatic parent creation:

```console
mka --no-auto dir1/dir2/file
```

In automatic mode, pairs of terminal `@` characters escape to literal `@`
characters. An unpaired terminal `@` marks a link target:

```console
mka name@@                 # creates the file name@
mka target@@@ link@@       # creates link@ pointing to target@
```

`-a`/`--auto` explicitly selects the default automatic mode. It conflicts with
`--no-auto` and `--type`.

### Parents

Missing parent directories are created by default. `--no-parents` instead
requires every parent to exist. A directory explicitly requested in the same
invocation can satisfy another operand's parent requirement:

```console
mka --no-parents new/ new/file
```

An intermediate symlink that resolves to a directory can be traversed. A
symlink is not accepted when the symlink itself is the requested directory leaf.

### Existing objects

`--no-exist-ok[=TYPE]` turns selected existing objects into errors:

| Option | Existing files | Existing directories | Existing links |
| --- | --- | --- | --- |
| omitted | accepted when requested | accepted when requested | accepted only with the same target |
| `--no-exist-ok` | error | default behavior | default behavior |
| `--no-exist-ok=file` | error | default behavior | default behavior |
| `--no-exist-ok=dir` | default behavior | error | default behavior |
| `--no-exist-ok=link` | default behavior | default behavior | error |
| `--no-exist-ok=any` | error | error | error |

The policy applies to every named path component, not only the leaf. Filesystem
roots, platform prefixes, `.` and `..` are excluded. When specifying a type, the
`=` is required so that bare `--no-exist-ok` can be followed immediately by a
path.

Wrong types and special filesystem entries are always errors. `mka` preflights
the complete request graph before making changes, so predictable errors do not
partially create an operand batch. A filesystem race after preflight can still
leave earlier successful creations in place.

### Moving existing objects

Use `-m` / `--move` to move files, directories, or symbolic links. Missing
destination directories are created automatically:

```console
mka -m report.pdf archive/2026/       # archive/2026/report.pdf
mka -m report.pdf archive/renamed.pdf # archive/renamed.pdf
mka -m a.txt b.txt new-folder/        # new-folder/a.txt and new-folder/b.txt
```

The last operand is the destination. With one source, a trailing separator or
an existing directory means “move into this directory”; otherwise it is the
exact new path. With multiple sources, the destination is always a directory,
even without a trailing separator. An existing symlink to a directory may be
used as a destination directory or intermediate parent.

Move mode treats `@` literally and conflicts with `--auto`, `--no-auto`, and
`--type`. Use `--` before operands that begin with a hyphen. A source symlink
is moved itself, including when dangling, and its stored target is preserved;
a relative target may resolve differently after the move. A trailing separator
on a source is accepted only for an actual directory.

`--no-parents` requires all destination directories to exist. `--no-exist-ok`
applies only to named destination components, never source components.
Every occupied final destination is an error, including a dangling symlink or
empty directory. Directories are moved whole; their contents are never merged.

The complete batch is checked for missing sources, collisions, overlapping
source directories, and destinations inside a moved directory before changes
begin. Parent aliases are resolved during planning. Native no-replace renames
also prevent overwriting a destination created after preflight. A runtime
failure stops the batch and may leave created directories and earlier moves
in place; there is no rollback.

Moves are supported on Linux, macOS, Windows, and FreeBSD where native
no-replace renames are available. Older FreeBSD installations, NetBSD, and
OpenBSD report unsupported moves. Unsupported filesystems and moves across
filesystems also produce errors: there is no copy-and-delete or non-atomic
fallback. Successful moves are silent.

### Shell completions

`mka --completion <shell>` prints a completion script to stdout. Supported shells
are `bash`, `elvish`, `fish`, `powershell`, and `zsh`. Use this option on its own;
it does not create filesystem objects.

```sh
# Load completions in the current Bash session
source <(mka --completion bash)

# Load completions in Zsh after initializing its completion system
autoload -Uz compinit && compinit
source <(mka --completion zsh)

# Install Fish completions
mkdir -p ~/.config/fish/completions
mka --completion fish > ~/.config/fish/completions/mka.fish
```

## Platform notes

Symbolic links may require operating-system privileges, notably Developer Mode
or equivalent permission on Windows. Symlink targets may be dangling. On
Windows, an existing target determines whether a file or directory link is
created. A missing target ending in a path separator is treated as a directory;
other missing targets are treated as files. Windows removes that trailing kind
marker from the stored symlink target because retaining it produces an invalid
reparse target; Unix stores the target exactly as written after `@` decoding.

Path syntax, permissions, and the final operating-system error text may differ
between platforms. Runtime failures exit with status 1, invalid command-line
syntax exits with status 2, and success exits with status 0.

## Install and develop

```console
cargo install --path .
cargo nextest run
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

The minimum supported Rust version is 1.98. Move operations use a pinned Git
revision of [our Renamore fork](https://github.com/mevanlc/renamore).
CI checks Rust 1.98 explicitly. OpenBSD 7.9 runtime tests use its older packaged
compiler with Cargo's `--ignore-rust-version` because an official Rust host
toolchain is unavailable there; this does not establish supported builds on
older Rust versions.

## Future ideas

- FIFOs
- Hard links
- macOS resource forks
- Extended attributes
- Windows alternate data streams
- Other platform-specific filesystem entries
