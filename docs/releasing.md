# Releasing

The repository ships two artifacts, versioned and released independently.

| Artifact                      | Version in           | Tags          |
| ----------------------------- | -------------------- | ------------- |
| the `c10r` binary             | `Cargo.toml`         | `v*`          |
| the `c10r-mcp` Python package | `mcp/pyproject.toml` | `c10r-mcp-v*` |

You cut the release locally in two steps: `just changelog` writes the new changelog section, you edit it, and `just release` bumps the version, commits, tags, and pushes.
GitHub Actions reacts to the tag: it runs the test suite against the tagged commit, then creates the GitHub release and attaches the built artifacts.
A failed test job leaves the tag with no release; fix the problem and re-run the workflow.

The release commit names its paths, so unrelated work in progress elsewhere in the tree neither blocks the release nor rides along in it.

The two version numbers are independent and may drift.
Compatibility between them is carried by `SURFACE_VERSION`, pinned on both sides in `src/manifest.rs` and `mcp/src/c10r_mcp/surface.py`: when the crate's command surface changes, that constant moves, the package's pin has to follow, and the package needs a release of its own.
Nothing else about a crate release requires one.

The harness in `evals/` is not released.

## Prerequisites

| Tool        | Needed for                                                            | Install                                                   |
| ----------- | --------------------------------------------------------------------- | --------------------------------------------------------- |
| `git`       | everything                                                            | your package manager                                      |
| `rustup`    | building the crate; installs the toolchain `rust-toolchain.toml` pins | <https://rustup.rs>                                       |
| `just`      | every command below                                                   | `cargo install just --locked`, or your package manager    |
| `uv`        | the MCP version bump and distributions                                | <https://docs.astral.sh/uv/getting-started/installation/> |
| `git-cliff` | both changelogs                                                       | `cargo install git-cliff --locked`                        |

`cargo install` puts these in `~/.cargo/bin`, which has to be on `PATH`.

## Cutting a release

`<package>` is `c10r` or `mcp`.
`<bump>` is `patch`, `minor`, `major`, or a version such as `1.2.0`.

```sh
just changelog c10r minor           # writes the 0.2.0 section into CHANGELOG.md
$EDITOR CHANGELOG.md                # rewrite the generated entries
just release c10r minor --dry-run   # what it would commit, tag, and push
just release c10r minor             # do it
```

The gap between the two commands is the point at which the generated entries get edited. git-cliff writes one line per commit subject; that is a starting draft, not the changelog.

`release` refuses unless the changelog already has a section for the version it is about to cut, so a skipped edit step fails loudly rather than publishing generated notes.
It also refuses off `main` and refuses a tag that already exists, and warns when the sources it is about to tag have uncommitted changes.
Dry runs change nothing.

The two commands must agree on the version, which they do because both resolve `<bump>` the same way — from the version currently in the manifest.
Editing the manifest version by hand between them would desynchronize the two.

## Settings

| File                                    | Holds                                                                                                |
| --------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| [`justfile`](../justfile)               | the release steps themselves, for both artifacts                                                     |
| [`.cliff.toml`](../.cliff.toml)         | git-cliff for the crate: `v*` tags, excluding commits that touch only `mcp/`, `evals/`, or `.specs/` |
| [`mcp/.cliff.toml`](../mcp/.cliff.toml) | git-cliff for the package: `c10r-mcp-v*` tags, only commits touching `mcp/`                          |

Each changelog is scoped to its own tag series and paths.
Without `tag_pattern`, the other artifact's tags would bound the commit range and truncate the changelog.

git-cliff is always invoked as `--unreleased --prepend`, which adds only the new section.
`--output` would rebuild the file from the commit history and discard every earlier edit.

The version bump goes through whichever tool owns the manifest: the crate's version is edited in place and `cargo metadata` propagates it into `Cargo.lock`, while `uv version` bumps and re-locks the package together.
Push is the last step, so a failure there leaves a local tag to delete before retrying.

## crates.io

Publishing is off; nothing in the release path runs `cargo publish`.
Enabling it needs a crates.io token and a publish step, and an offline path in [`build.rs`](../build.rs), which downloads the embedding model on every build; docs.rs builds without network access.

## Continuous integration

[`ci-rust.yaml`](../.github/workflows/ci-rust.yaml) runs `cargo fmt --check`, `cargo clippy -D warnings`, and `cargo test`.
[`ci-python.yaml`](../.github/workflows/ci-python.yaml) runs pytest for `mcp/`, and ruff and pytest for `evals/`; it builds `c10r` first, for the MCP tests.

Both run on pull requests and pushes to `main`, and the CD workflows call them.
`just check-rust`, `just check-python`, and `just check-hooks` run the same checks locally.
