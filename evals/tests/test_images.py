"""Treatment image builds: tag normalization, partial failure, record append, docker argv, rebuild decisions."""

import json
import subprocess

from c10r_evals import images
from c10r_evals.images import build_images, env_digest, image_tag


def _make_task(tree, instance, dockerfile="FROM scratch\n"):
    task_dir = tree / instance
    (task_dir / "environment").mkdir(parents=True)
    (task_dir / "task.toml").write_text("")
    (task_dir / "environment" / "Dockerfile").write_text(dockerfile)
    return task_dir


def _is_build(argv):
    return argv[:3] == ["docker", "buildx", "build"]


def _fake_docker(calls, *, image_present=True, stdout="0\n"):
    """Stand in for docker: `image inspect` reports presence, everything else succeeds."""

    def fake_run(argv, **kwargs):
        calls.append(argv)
        if argv[1] == "image":
            return subprocess.CompletedProcess(argv, 0 if image_present else 1)
        return subprocess.CompletedProcess(argv, 0, stdout=stdout)

    return fake_run


def test_tag_normalization_folds_disallowed_characters():
    """Docker tags are lowercase; every character outside `a-z0-9._-` folds to one '-'."""
    assert image_tag("Foo__Bar!!Baz.py") == "c10r-eval-treatment-foo__bar--baz.py"


def test_env_digest_covers_the_dockerfile_and_the_injected_binary(tmp_path):
    """Regenerating, re-injecting, or rebuilding the binary all change `environment/`."""
    task_dir = _make_task(tmp_path / "tree", "inst-a")
    original = env_digest(task_dir)

    (task_dir / "environment" / "Dockerfile").write_text("FROM scratch\nRUN echo regenerated\n")
    assert env_digest(task_dir) != original

    (task_dir / "environment" / "Dockerfile").write_text("FROM scratch\n")
    assert env_digest(task_dir) == original

    (task_dir / "environment" / "c10r").write_bytes(b"\x7fELF-new-binary")
    assert env_digest(task_dir) != original


def test_an_unchanged_task_with_its_image_present_is_not_rebuilt(tmp_path, monkeypatch):
    tree = tmp_path / "tree"
    task_dir = _make_task(tree, "inst-a")
    calls: list[list[str]] = []
    monkeypatch.setattr(images.subprocess, "run", _fake_docker(calls))
    records = tmp_path / "records.jsonl"

    build_images(tree, records, "linux/arm64", "c10r", built={"inst-a": env_digest(task_dir)})

    assert [argv[1] for argv in calls] == ["image"], "a built image must not be rebuilt or re-measured"
    assert not records.exists() or records.read_text() == ""


def test_a_task_whose_environment_changed_is_rebuilt_despite_its_image(tmp_path, monkeypatch):
    """A re-injected or regenerated task must not be trusted because an image of its old self exists."""
    tree = tmp_path / "tree"
    _make_task(tree, "inst-a")
    calls: list[list[str]] = []
    monkeypatch.setattr(images.subprocess, "run", _fake_docker(calls))

    build_images(
        tree, tmp_path / "records.jsonl", "linux/arm64", "c10r", built={"inst-a": "a-digest-from-an-older-build"}
    )

    assert any(_is_build(argv) for argv in calls)


def test_a_recorded_task_whose_image_is_gone_is_rebuilt(tmp_path, monkeypatch):
    tree = tmp_path / "tree"
    task_dir = _make_task(tree, "inst-a")
    calls: list[list[str]] = []
    monkeypatch.setattr(images.subprocess, "run", _fake_docker(calls, image_present=False))

    build_images(tree, tmp_path / "records.jsonl", "linux/arm64", "c10r", built={"inst-a": env_digest(task_dir)})

    assert any(_is_build(argv) for argv in calls)


def test_the_digest_is_recorded_so_a_later_run_can_skip_the_build(tmp_path, monkeypatch):
    tree = tmp_path / "tree"
    task_dir = _make_task(tree, "inst-a")
    monkeypatch.setattr(images.subprocess, "run", _fake_docker([]))
    records = tmp_path / "records.jsonl"

    build_images(tree, records, "linux/arm64", "c10r")

    assert json.loads(records.read_text())["env_digest"] == env_digest(task_dir)


def test_a_failed_build_is_skipped_and_the_rest_are_still_recorded(tmp_path, monkeypatch):
    tree = tmp_path / "tree"
    for instance in ("inst-a", "inst-b", "inst-c"):
        _make_task(tree, instance)

    def fake_run(argv, **kwargs):
        if _is_build(argv):
            if image_tag("inst-b") in argv:
                raise subprocess.CalledProcessError(1, argv)
            return subprocess.CompletedProcess(argv, 0)
        return subprocess.CompletedProcess(argv, 0, stdout="4096\n")

    monkeypatch.setattr(images.subprocess, "run", fake_run)
    records = tmp_path / "records.jsonl"

    failed = build_images(tree, records, "linux/arm64", "c10r")

    assert failed == ["inst-b"]
    recorded = [json.loads(line) for line in records.read_text().splitlines()]
    assert [r["instance_id"] for r in recorded] == ["inst-a", "inst-c"]


def test_records_are_appended_to_an_existing_file_rather_than_truncated(tmp_path, monkeypatch):
    tree = tmp_path / "tree"
    _make_task(tree, "inst-a")
    records = tmp_path / "records.jsonl"
    preexisting = json.dumps({"instance_id": "old-inst", "wall_clock_s": 5, "index_bytes": 1})
    records.write_text(preexisting + "\n")

    monkeypatch.setattr(
        images.subprocess, "run", lambda argv, **kw: subprocess.CompletedProcess(argv, 0, stdout="10\n")
    )

    build_images(tree, records, "linux/arm64", "c10r")

    lines = records.read_text().splitlines()
    assert lines[0] == preexisting
    assert json.loads(lines[1])["instance_id"] == "inst-a"


def test_docker_argv_matches_the_build_and_measure_commands(tmp_path, monkeypatch):
    tree = tmp_path / "tree"
    _make_task(tree, "inst-a")
    calls: list[tuple[list[str], dict]] = []

    def fake_run(argv, **kwargs):
        calls.append((argv, kwargs))
        return subprocess.CompletedProcess(argv, 0, stdout="0\n")

    monkeypatch.setattr(images.subprocess, "run", fake_run)

    build_images(tree, tmp_path / "records.jsonl", "linux/arm64", "c10r")

    tag = image_tag("inst-a")
    build_argv, build_kwargs = calls[0]
    # `docker build` does not follow buildx's current selection, so the builder is named here.
    assert build_argv == [
        "docker",
        "buildx",
        "build",
        "--builder",
        "c10r",
        "--platform",
        "linux/arm64",
        "--load",
        "-t",
        tag,
        str(tree / "inst-a" / "environment"),
    ]
    assert build_kwargs["stdout"] == subprocess.DEVNULL
    assert build_kwargs["check"] is True
    assert "stderr" not in build_kwargs  # inherited from the parent, not redirected

    run_argv, run_kwargs = calls[1]
    assert run_argv == [
        "docker",
        "run",
        "--rm",
        "--platform",
        "linux/arm64",
        tag,
        "sh",
        "-c",
        "du -sb /repo/.c10r 2>/dev/null | cut -f1 || echo 0",
    ]
    assert run_kwargs["stdout"] == subprocess.PIPE
    assert run_kwargs["text"] is True
