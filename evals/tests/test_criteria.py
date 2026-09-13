import importlib

from c10r_evals.analysis import Criteria as AnalysisCriteria


def test_criteria_module_is_canonical_import_path():
    criteria = importlib.import_module("c10r_evals.criteria")

    assert criteria.Criteria is AnalysisCriteria
