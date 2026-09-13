"""Tracked Python source paths for a repository at a pinned commit.

Pre-run task characteristics (source-file count, source-file cue) need the repository's tracked
paths at the instance's base commit, without paying for a full checkout. A `--filter=tree:0`
clone fetches commit and tree metadata but not blob contents, and `git ls-tree` reads the tracked
paths from that metadata alone. The clone is cached per repository so a repeat draw, or a second
instance from the same repository, costs no network.

This is the only module in task generation that touches the network. `taskgen.py` stays a pure
function of a resolved instance and its already-computed characteristics so its tests stay
hermetic; the CLI resolves tracked paths here and passes them in.

The clones are a build-time intermediate, not an artifact: generation writes the characteristics
it derives from them into each task, and nothing reads the clones afterwards, since a treatment
image clones its own repository. Replay comes from the pinned dataset revision and base commit,
which reproduce the same tree, so `clone_cache` discards the clones when generation finishes.
Set `C10R_EVALS_REPO_TREE_CACHE` to keep them, which is worth doing while iterating on the
characteristics themselves: re-cloning every repository of a subset on each run is slow.
"""

import contextlib
import logging
import os
import subprocess
import tempfile
from collections.abc import Iterator
from pathlib import Path

from c10r_evals.runtime import log_fields

logger = logging.getLogger(__name__)

CACHE_DIR_ENV = "C10R_EVALS_REPO_TREE_CACHE"


class RepoTreeError(Exception):
    """Cloning a repository or reading its tracked tree failed; the caller cannot proceed."""


@contextlib.contextmanager
def clone_cache() -> Iterator[Path]:
    """Yield the directory the treeless clones live in for one generation run.

    Yields the `C10R_EVALS_REPO_TREE_CACHE` directory when that is set, keeping the clones across
    runs; otherwise yields a temporary directory removed on exit. One directory serves a whole
    run either way, so a subset drawing many instances from one repository still clones it once.
    """
    override = os.environ.get(CACHE_DIR_ENV)
    if override:
        root = Path(override)
        root.mkdir(parents=True, exist_ok=True)
        log_fields(logger, logging.INFO, "repo_tree_cache_persistent", cache_dir=str(root))
        yield root
        return
    with tempfile.TemporaryDirectory(prefix="c10r-evals-repo-trees-") as tmp:
        log_fields(logger, logging.INFO, "repo_tree_cache_temporary", cache_dir=tmp)
        yield Path(tmp)


def _run_git(args: list[str]) -> subprocess.CompletedProcess:
    return subprocess.run(["git", *args], capture_output=True, text=True, check=False)


def _clone(repo: str, clone_dir: Path) -> None:
    clone_dir.parent.mkdir(parents=True, exist_ok=True)
    # credential.helper is cleared: without it, an unauthenticated clone on this machine fails
    # with a macOS Keychain error (`fatal: failed to store: -60008`) instead of cloning anonymously.
    result = _run_git(
        [
            "-c",
            "credential.helper=",
            "clone",
            "--filter=tree:0",
            "--no-checkout",
            f"https://github.com/{repo}.git",
            str(clone_dir),
        ]
    )
    if result.returncode != 0:
        raise RepoTreeError(f"cloning {repo} into {clone_dir} failed: {result.stderr.strip()}")


def _ls_tree(clone_dir: Path, base_commit: str) -> subprocess.CompletedProcess:
    return _run_git(["-C", str(clone_dir), "ls-tree", "-r", "--name-only", base_commit])


def _fetch_commit(clone_dir: Path, base_commit: str) -> None:
    result = _run_git(["-C", str(clone_dir), "fetch", "--filter=tree:0", "origin", base_commit])
    if result.returncode != 0:
        raise RepoTreeError(f"fetching {base_commit} into {clone_dir} failed: {result.stderr.strip()}")


def tracked_python_paths(repo: str, base_commit: str, cache_dir: Path) -> list[str]:
    """Tracked `.py` paths at `base_commit`, from a treeless clone of `repo` under `cache_dir`.

    Clones into `cache_dir` on first use for this repository; a populated cache needs no network
    for a commit already reachable from a fetched ref. If `ls-tree` cannot resolve the commit,
    fetches it directly and retries once, then raises with git's own error if that also fails.

    The caller supplies `cache_dir` and owns its lifetime, normally through `clone_cache`.
    """
    clone_dir = cache_dir / repo
    if not clone_dir.is_dir():
        log_fields(logger, logging.INFO, "repo_tree_clone", repo=repo, cache_dir=str(clone_dir))
        _clone(repo, clone_dir)

    result = _ls_tree(clone_dir, base_commit)
    if result.returncode != 0:
        log_fields(logger, logging.INFO, "repo_tree_fetch_commit", repo=repo, base_commit=base_commit)
        _fetch_commit(clone_dir, base_commit)
        result = _ls_tree(clone_dir, base_commit)
        if result.returncode != 0:
            raise RepoTreeError(
                f"reading tracked tree for {repo}@{base_commit} failed after fetch: {result.stderr.strip()}"
            )

    return sorted(path for path in result.stdout.splitlines() if path.endswith(".py"))
