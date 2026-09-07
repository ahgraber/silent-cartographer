"""Grading totality, accuracy scoring, symbol neutrality, and path normalization.

Tests import the packaged grader source directly — the same file task generation
copies into every task's verifier directory.
"""

import json
import subprocess
import sys
from pathlib import Path

from c10r_evals.verifier_grade import grade, normalize_path, parse_answer, score

GOLD = ["pkg/core.py", "pkg/util/paths.py"]


# --- Grading totality: well-formed, malformed, and missing answers all produce a reward.


def test_grading_wellformed():
    result = grade(json.dumps({"files": ["pkg/core.py"]}), GOLD)
    assert result["unparsed"] is False
    assert result["files"] == ["pkg/core.py"]
    assert result["any_gold_hit"] == 1


def test_grading_malformed_empty_flagged():
    result = grade("this is not json at all", GOLD)
    assert result["unparsed"] is True
    assert result["files"] == []
    assert result["precision"] == result["recall"] == result["f1"] == 0.0
    assert result["any_gold_hit"] == 0


def test_grading_missing_empty_flagged():
    result = grade(None, GOLD)
    assert result["unparsed"] is True
    assert result["files"] == []
    assert result["reward"] == 0.0


def test_verifier_exits_zero_end_to_end(tmp_path):
    """The grader script itself: missing answer file, still writes a reward and exits 0."""
    verifier_dir = tmp_path / "verifier"
    verifier_dir.mkdir()
    grader = verifier_dir / "grade.py"
    grader.write_text(Path("src/c10r_evals/verifier_grade.py").read_text())
    (verifier_dir / "gold.json").write_text(json.dumps({"gold_files": GOLD}))

    env = {"ANSWER_PATH": str(tmp_path / "does-not-exist.json"), "PATH": "/usr/bin:/bin"}
    proc = subprocess.run([sys.executable, str(grader)], env=env, capture_output=True, check=False)
    assert proc.returncode == 0
    reward = json.loads((verifier_dir / "reward.json").read_text())
    assert all(isinstance(v, (int, float)) for v in reward.values())  # Pier's flat-numeric contract
    assert reward["unparsed"] == 1
    assert reward["reward"] == 0.0
    details = json.loads((verifier_dir / "grade-details.json").read_text())
    assert details["files"] == []


def test_salvage_json_from_prose():
    text = 'Here is my answer:\n{"files": ["pkg/core.py"]}\nDone.'
    files, unparsed, _ = parse_answer(text)
    assert unparsed is False
    assert files == ["pkg/core.py"]


# --- Accuracy scoring partitions.


def test_score_exact_match():
    result = score(GOLD, GOLD)
    assert result["precision"] == 1.0
    assert result["recall"] == 1.0
    assert result["f1"] == 1.0
    assert result["any_gold_hit"] == 1


def test_score_partial_overlap():
    result = score(["pkg/core.py", "unrelated.py"], GOLD)
    assert result["precision"] == 0.5
    assert result["recall"] == 0.5
    assert result["f1"] == 0.5
    assert result["any_gold_hit"] == 1


def test_score_disjoint():
    result = score(["other.py"], GOLD)
    assert result["precision"] == 0.0
    assert result["recall"] == 0.0
    assert result["f1"] == 0.0
    assert result["any_gold_hit"] == 0


# --- Symbol neutrality and path normalization.


def test_symbols_do_not_change_reward():
    plain = grade(json.dumps({"files": ["pkg/core.py"]}), GOLD)
    with_symbols = grade(json.dumps({"files": ["pkg/core.py"], "symbols": {"pkg/core.py": ["handler"]}}), GOLD)
    for key in ("reward", "precision", "recall", "f1", "any_gold_hit", "unparsed", "files"):
        assert plain[key] == with_symbols[key]
    assert with_symbols["symbols_present"] is True
    assert plain["symbols_present"] is False


def test_path_normalization_equivalence():
    normalized = grade(json.dumps({"files": ["pkg/core.py"]}), GOLD)
    messy = grade(json.dumps({"files": ["./pkg/core.py", "pkg//util/../util/paths.py"]}), GOLD)
    assert messy["files"] == ["pkg/core.py", "pkg/util/paths.py"]
    assert messy["any_gold_hit"] == 1
    assert normalize_path("/pkg/core.py") == "pkg/core.py"
    assert normalized["files"] == ["pkg/core.py"]
