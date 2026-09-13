"""Tracked-path resolution: cache reuse, fetch-and-retry fallback, and explicit failure.

Every git invocation is faked via a stub on `subprocess.run` so these tests never touch the
network, matching the constraint that only `repotree.py` (never `taskgen.py`) may.
"""

import subprocess
from pathlib import Path

import pytest

from c10r_evals.repotree import RepoTreeError, clone_cache, tracked_python_paths

REPO = "demo/repo"
BASE_COMMIT = "a" * 40


def _completed(returncode: int, stdout: str = "", stderr: str = "") -> subprocess.CompletedProcess:
    return subprocess.CompletedProcess(args=[], returncode=returncode, stdout=stdout, stderr=stderr)


def test_clones_once_then_reuses_cache(tmp_path, monkeypatch):
    """A populated cache is reused: the second call issues no clone invocation."""
    calls: list[list[str]] = []

    def fake_run(args, **kwargs):
        calls.append(args)
        if "clone" in args:
            (tmp_path / REPO).mkdir(parents=True)
            return _completed(0)
        if "ls-tree" in args:
            return _completed(0, stdout="pkg/core.py\npkg/util/paths.py\nREADME.md\n")
        raise AssertionError(f"unexpected git invocation: {args}")

    monkeypatch.setattr(subprocess, "run", fake_run)

    first = tracked_python_paths(REPO, BASE_COMMIT, cache_dir=tmp_path)
    second = tracked_python_paths(REPO, BASE_COMMIT, cache_dir=tmp_path)

    assert first == second == ["pkg/core.py", "pkg/util/paths.py"]
    clone_calls = [c for c in calls if "clone" in c]
    assert len(clone_calls) == 1


def test_unreachable_commit_fetches_then_retries(tmp_path, monkeypatch):
    """`ls-tree` failing on an unfetched commit triggers a direct fetch, then one retry."""
    (tmp_path / REPO).mkdir(parents=True)
    ls_tree_calls = 0

    def fake_run(args, **kwargs):
        nonlocal ls_tree_calls
        if "fetch" in args:
            return _completed(0)
        if "ls-tree" in args:
            ls_tree_calls += 1
            if ls_tree_calls == 1:
                return _completed(128, stderr="fatal: Not a valid object name")
            return _completed(0, stdout="pkg/core.py\n")
        raise AssertionError(f"unexpected git invocation: {args}")

    monkeypatch.setattr(subprocess, "run", fake_run)

    result = tracked_python_paths(REPO, BASE_COMMIT, cache_dir=tmp_path)

    assert result == ["pkg/core.py"]
    assert ls_tree_calls == 2


def test_persistent_failure_raises_with_git_stderr(tmp_path, monkeypatch):
    (tmp_path / REPO).mkdir(parents=True)

    def fake_run(args, **kwargs):
        if "fetch" in args:
            return _completed(0)
        if "ls-tree" in args:
            return _completed(128, stderr="fatal: bad object deadbeef")
        raise AssertionError(f"unexpected git invocation: {args}")

    monkeypatch.setattr(subprocess, "run", fake_run)

    with pytest.raises(RepoTreeError, match="bad object deadbeef"):
        tracked_python_paths(REPO, BASE_COMMIT, cache_dir=tmp_path)


def test_clone_failure_raises_with_git_stderr(tmp_path, monkeypatch):
    def fake_run(args, **kwargs):
        if "clone" in args:
            return _completed(128, stderr="fatal: repository not found")
        raise AssertionError(f"unexpected git invocation: {args}")

    monkeypatch.setattr(subprocess, "run", fake_run)

    with pytest.raises(RepoTreeError, match="repository not found"):
        tracked_python_paths(REPO, BASE_COMMIT, cache_dir=tmp_path)


def test_clone_cache_discards_its_directory_by_default(monkeypatch):
    """The clones are a build-time intermediate, so an unconfigured run leaves nothing behind."""
    monkeypatch.delenv("C10R_EVALS_REPO_TREE_CACHE", raising=False)

    with clone_cache() as cache_dir:
        (cache_dir / "clone-marker").write_text("")
        assert cache_dir.is_dir()
        leaked = cache_dir

    assert not leaked.exists()


def test_clone_cache_keeps_the_configured_directory(monkeypatch, tmp_path):
    """Setting the cache directory keeps the clones, so iterating does not re-clone every repo."""
    configured = tmp_path / "kept"
    monkeypatch.setenv("C10R_EVALS_REPO_TREE_CACHE", str(configured))

    with clone_cache() as cache_dir:
        assert cache_dir == configured
        (cache_dir / "clone-marker").write_text("")

    assert (configured / "clone-marker").is_file()


def test_one_cache_serves_repeated_repositories_in_a_run(monkeypatch, tmp_path):
    """Instances sharing a repository clone it once: the cost is per repository, not per instance."""
    monkeypatch.delenv("C10R_EVALS_REPO_TREE_CACHE", raising=False)
    clones: list[str] = []

    def fake_run(args, **kwargs):
        if "clone" in args:
            clones.append(args[args.index("clone") + 3])
            Path(args[-1]).mkdir(parents=True)
            return subprocess.CompletedProcess(args, 0, "", "")
        return subprocess.CompletedProcess(args, 0, "pkg/mod.py\nREADME.md\n", "")

    monkeypatch.setattr(subprocess, "run", fake_run)

    with clone_cache() as cache_dir:
        for _ in range(3):
            tracked_python_paths(REPO, BASE_COMMIT, cache_dir=cache_dir)

    assert len(clones) == 1
