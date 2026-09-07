"""Standalone localization grader, copied verbatim into each task's verifier directory.

Runs inside the task container with only the standard library.
Reads the agent's JSON answer and the task's gold file set, scores historical-fix
file recovery, and always writes a reward — a missing, malformed, or unparseable
answer scores as the empty set with an `unparsed` flag and a zero exit.
"""

import json
import os
import posixpath
import sys

ANSWER_PATH = "/answer.json"


def normalize_path(path: str) -> str:
    """Compare answer and gold paths repo-root-relative regardless of spelling."""
    cleaned = path.strip().lstrip("/")
    cleaned = posixpath.normpath(cleaned)
    return "" if cleaned == "." else cleaned


def salvage_json(text: str) -> dict | None:
    """Parse `text` as JSON; if that fails, try the outermost brace-delimited block."""
    try:
        parsed = json.loads(text)
        return parsed if isinstance(parsed, dict) else None
    except (json.JSONDecodeError, ValueError):
        pass
    start, end = text.find("{"), text.rfind("}")
    if start == -1 or end <= start:
        return None
    try:
        parsed = json.loads(text[start : end + 1])
        return parsed if isinstance(parsed, dict) else None
    except (json.JSONDecodeError, ValueError):
        return None


def parse_answer(text: str | None) -> tuple[list[str], bool, bool]:
    """Return (normalized answer files, unparsed flag, symbols present)."""
    if text is None:
        return [], True, False
    obj = salvage_json(text)
    if obj is None:
        return [], True, False
    files = obj.get("files")
    if not isinstance(files, list) or not all(isinstance(f, str) for f in files):
        return [], True, False
    normalized: dict[str, None] = {}
    for f in files:
        cleaned = normalize_path(f)
        if cleaned:
            normalized.setdefault(cleaned)
    symbols = obj.get("symbols")
    has_symbols = isinstance(symbols, dict) and bool(symbols)
    return list(normalized), False, has_symbols


def score(answer_files: list[str], gold_files: list[str]) -> dict:
    """File-level precision/recall/F1 and any-gold-file hit; symbols never contribute."""
    answer = set(answer_files)
    gold = {normalize_path(g) for g in gold_files}
    overlap = len(answer & gold)
    precision = overlap / len(answer) if answer else 0.0
    recall = overlap / len(gold) if gold else 0.0
    f1 = (2 * precision * recall / (precision + recall)) if (precision + recall) else 0.0
    return {
        "precision": precision,
        "recall": recall,
        "f1": f1,
        "any_gold_hit": 1 if overlap else 0,
    }


def grade(answer_text: str | None, gold_files: list[str]) -> dict:
    answer_files, unparsed, has_symbols = parse_answer(answer_text)
    result = score(answer_files, gold_files)
    result.update(
        {
            "reward": result["f1"],
            "unparsed": unparsed,
            "files": answer_files,
            "symbols_present": has_symbols,
        }
    )
    return result


def main() -> int:
    verifier_dir = os.path.dirname(os.path.abspath(__file__))
    with open(os.path.join(verifier_dir, "gold.json"), encoding="utf-8") as fh:
        gold_files = json.load(fh)["gold_files"]
    answer_path = os.environ.get("ANSWER_PATH", ANSWER_PATH)
    answer_text = None
    if os.path.exists(answer_path):
        try:
            with open(answer_path, encoding="utf-8", errors="replace") as fh:
                answer_text = fh.read()
        except OSError:
            answer_text = None
    result = grade(answer_text, gold_files)
    reward_path = os.environ.get("REWARD_PATH")
    if reward_path is None:
        # Pier mounts the trial's verifier output dir at /logs/verifier inside the
        # environment; fall back next to this script when running outside a trial.
        verifier_logs = os.environ.get("ENV_VERIFIER_LOGS_PATH", "/logs/verifier")
        target_dir = verifier_logs if os.path.isdir(verifier_logs) else verifier_dir
        reward_path = os.path.join(target_dir, "reward.json")
    # Pier requires the reward file to be a flat mapping of numbers.
    numeric_reward = {
        "reward": result["reward"],
        "precision": result["precision"],
        "recall": result["recall"],
        "f1": result["f1"],
        "any_gold_hit": result["any_gold_hit"],
        "unparsed": 1 if result["unparsed"] else 0,
        "symbols_present": 1 if result["symbols_present"] else 0,
    }
    with open(reward_path, "w", encoding="utf-8") as fh:
        json.dump(numeric_reward, fh, indent=2)
    details_path = os.path.join(os.path.dirname(reward_path), "grade-details.json")
    with open(details_path, "w", encoding="utf-8") as fh:
        json.dump(result, fh, indent=2)
    return 0


if __name__ == "__main__":
    sys.exit(main())
