# Release runbook. Every recipe runs from the repository root; `just` lists them by group.
#
# Releasing takes two steps, the same for both artifacts:
#
#   just changelog <package> <bump>   writes the new section; you edit it
#   just release   <package> <bump>   bumps, commits, tags, pushes
#
# The gap between them is the point at which the generated entries get rewritten by hand.
# `release` refuses if the changelog has no section for the version it is about to cut.
#
# Commits name their paths explicitly, so unrelated work in progress elsewhere in the tree
# neither blocks a release nor rides along in one. See docs/releasing.md.
#
# `package` is `c10r` or `mcp`. `bump` is `patch`, `minor`, `major`, or an explicit
# version such as `1.2.0`.

# List the available recipes.
default:
    @just --list

# --- release -----------------------------------------------------------------

# The version `bump` resolves to, without applying it.
[private]
next-version package bump:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ "{{ bump }}" =~ ^[0-9]+\.[0-9]+\.[0-9]+ ]]; then
        echo "{{ bump }}"
        exit 0
    fi
    case "{{ package }}" in
        c10r)
            current="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
            IFS=. read -r major minor patch <<<"${current}"
            case "{{ bump }}" in
                major) echo "$((major + 1)).0.0" ;;
                minor) echo "${major}.$((minor + 1)).0" ;;
                patch) echo "${major}.${minor}.$((patch + 1))" ;;
                *) echo "unknown bump: {{ bump }} (expected patch, minor, major, or a version)" >&2; exit 2 ;;
            esac
            ;;
        mcp)
            uv version --directory mcp --short --dry-run --bump "{{ bump }}"
            ;;
        *)
            echo "unknown package: {{ package }} (expected c10r or mcp)" >&2
            exit 2
            ;;
    esac

# Write the next version's changelog section, for editing before the release.
[group('release')]
changelog package bump:
    #!/usr/bin/env bash
    set -euo pipefail
    version="$({{ just_executable() }} next-version {{ package }} {{ bump }})"
    case "{{ package }}" in
        c10r)
            changelog=CHANGELOG.md
            git-cliff --tag "v${version}" --unreleased --prepend "${changelog}"
            ;;
        mcp)
            changelog=mcp/CHANGELOG.md
            git-cliff --config mcp/.cliff.toml --tag "c10r-mcp-v${version}" \
                --unreleased --prepend "${changelog}"
            ;;
        *)
            echo "unknown package: {{ package }} (expected c10r or mcp)" >&2
            exit 2
            ;;
    esac
    # Normalize now, so the file being edited is already what the commit hooks accept.
    if command -v prek >/dev/null; then
        prek run --files "${changelog}" >/dev/null 2>&1 || true
    fi
    echo
    echo "wrote the ${version} section to ${changelog}."
    echo "edit it, then: just release {{ package }} {{ bump }}"

# Pass --dry-run to report what would happen without changing anything.
# Cut a release: bump the version, commit, tag, push. Run `just changelog` first.
[group('release')]
release package bump *flags:
    #!/usr/bin/env bash
    set -euo pipefail

    dry_run=false
    for flag in {{ flags }}; do
        case "${flag}" in
            --dry-run) dry_run=true ;;
            *) echo "unknown flag: ${flag} (expected --dry-run)" >&2; exit 2 ;;
        esac
    done

    version="$({{ just_executable() }} next-version {{ package }} {{ bump }})"
    case "{{ package }}" in
        c10r)
            tag="v${version}"
            changelog=CHANGELOG.md
            previous="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
            sources=(src build.rs)
            ;;
        mcp)
            tag="c10r-mcp-v${version}"
            changelog=mcp/CHANGELOG.md
            previous="$(uv version --directory mcp --short)"
            sources=(mcp/src)
            ;;
        *)
            echo "unknown package: {{ package }} (expected c10r or mcp)" >&2
            exit 2
            ;;
    esac

    # Preflight, before anything is written.
    if [[ "$(git rev-parse --abbrev-ref HEAD)" != main ]]; then
        echo "releases are cut from main" >&2; exit 1
    fi
    if git rev-parse -q --verify "refs/tags/${tag}" >/dev/null; then
        echo "tag ${tag} already exists" >&2; exit 1
    fi
    # The release body comes from this section, so a missing one means the edit step
    # was skipped and the published notes would fall back to generated ones.
    escaped="${version//./\\.}"
    if ! grep -qE "^## .*[^0-9.]${escaped}([^0-9.]|\$)" "${changelog}"; then
        echo "${changelog} has no section for ${version}" >&2
        echo "run: just changelog {{ package }} {{ bump }}" >&2
        exit 1
    fi
    if [[ -n "$(git status --porcelain -- "${sources[@]}")" ]]; then
        echo "warning: uncommitted changes under ${sources[*]} will not be in ${tag}" >&2
    fi

    if [[ "${dry_run}" == true ]]; then
        echo "{{ package }} ${previous} → ${version}, tagged ${tag} at $(git rev-parse --short HEAD)"
        echo "release body: the ${version} section of ${changelog}"
        exit 0
    fi

    # Settle the formatters before anything is bumped. A hook that rewrites a file
    # during the commit aborts it, and the bump would already have been applied —
    # after which `next-version` reads the bumped manifest and resolves one version
    # too far on the retry.
    if command -v prek >/dev/null; then
        prek run --files "${changelog}" >/dev/null 2>&1 || true
    fi

    case "{{ package }}" in
        c10r)
            sed -i.tmp "0,/^version = \".*\"/s//version = \"${version}\"/" Cargo.toml
            rm -f Cargo.toml.tmp
            # Propagates the new version into Cargo.lock's own entry.
            cargo metadata --offline --format-version 1 >/dev/null
            manifests=(Cargo.toml Cargo.lock)
            ;;
        mcp)
            # `uv version` re-locks, so pyproject.toml and uv.lock move together.
            uv version --directory mcp "${version}" >/dev/null
            manifests=(mcp/pyproject.toml mcp/uv.lock)
            ;;
    esac

    # Naming the paths keeps unrelated work in progress out of the release commit.
    # `-m` has to precede `--`; everything after it is read as a pathspec.
    if ! git commit -m "bump({{ package }}): ${previous} → ${version}" \
        -- "${manifests[@]}" "${changelog}"; then
        # Undo the bump so a retry resolves the same version again. The changelog
        # keeps its section, so the editing work survives.
        git checkout HEAD -- "${manifests[@]}"
        echo "commit failed; rolled the version back to ${previous}" >&2
        exit 1
    fi
    git tag -a "${tag}" -m "{{ package }} ${version}"
    # Push last: a failure here leaves a local tag to delete and retry.
    git push origin main "${tag}"

# --- checks ------------------------------------------------------------------

# Run what CI (Rust) runs.
[group('check')]
check-rust:
    cargo fmt --all --check
    cargo clippy --release --all-targets --all-features -- -D warnings
    cargo test --release

# Run what CI (Python) runs. Needs `cargo build --release` first for the MCP tests.
[group('check')]
check-python:
    cd mcp && uv run --frozen pytest
    cd evals && uv run --frozen ruff check .
    cd evals && uv run --frozen pytest

# Run every pre-commit hook over the whole tree.
[group('check')]
check-hooks:
    prek run --all-files
