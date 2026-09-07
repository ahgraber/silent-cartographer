"""Gold answer derivation from instance diffs."""

from conftest import MULTI_FILE_PATCH, SINGLE_FILE_PATCH, TEST_PATCH, make_instance

from c10r_evals.gold import parse_changed_files


def test_gold_single_file():
    assert parse_changed_files(SINGLE_FILE_PATCH) == ["pkg/core.py"]


def test_gold_multi_file():
    assert parse_changed_files(MULTI_FILE_PATCH) == ["pkg/core.py", "pkg/util/paths.py"]


def test_gold_excludes_tests():
    instance = make_instance()
    gold = parse_changed_files(instance.patch)
    test_files = parse_changed_files(TEST_PATCH)
    assert test_files == ["tests/test_core.py"]
    assert not set(gold) & set(test_files)
