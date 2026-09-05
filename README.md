# mka

`mka` means “mk anything.” It creates regular files, directories, missing parent
directories, and symbolic links through one compact command. It supports macOS,
Linux, Windows, FreeBSD, NetBSD, and OpenBSD.

Success is silent. Creating an object that already has the requested type is
normally a no-op: existing regular files are not truncated or touched, existing
directories are accepted, and existing symbolic links are accepted only when
their stored target is exactly the requested target.

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

The minimum supported Rust version is 1.85.

## Future ideas

- FIFOs
- Hard links
- macOS resource forks
- Extended attributes
- Windows alternate data streams
- Other platform-specific filesystem entries
