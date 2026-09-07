"""Shared fixtures: synthetic dataset instances with known diffs."""

import pytest

from c10r_evals.dataset import Instance

SINGLE_FILE_PATCH = """\
diff --git a/pkg/core.py b/pkg/core.py
index 1111111..2222222 100644
--- a/pkg/core.py
+++ b/pkg/core.py
@@ -1,3 +1,4 @@
 def handler():
-    return None
+    return 1
"""

MULTI_FILE_PATCH = """\
diff --git a/pkg/core.py b/pkg/core.py
index 1111111..2222222 100644
--- a/pkg/core.py
+++ b/pkg/core.py
@@ -1,3 +1,4 @@
 def handler():
-    return None
+    return 1
diff --git a/pkg/util/paths.py b/pkg/util/paths.py
index 3333333..4444444 100644
--- a/pkg/util/paths.py
+++ b/pkg/util/paths.py
@@ -10,1 +10,2 @@
+NEW = True
"""

TEST_PATCH = """\
diff --git a/tests/test_core.py b/tests/test_core.py
index 5555555..6666666 100644
--- a/tests/test_core.py
+++ b/tests/test_core.py
@@ -1,1 +1,2 @@
+def test_handler(): pass
"""


def make_instance(instance_id: str = "demo__repo-1", patch: str = SINGLE_FILE_PATCH) -> Instance:
    return Instance(
        instance_id=instance_id,
        repo="demo/repo",
        base_commit="a" * 40,
        problem_statement=f"Crash in handler for {instance_id}: the handler returns None on valid input.",
        patch=patch,
        test_patch=TEST_PATCH,
    )


@pytest.fixture
def instances() -> list[Instance]:
    return [
        make_instance("demo__repo-1", SINGLE_FILE_PATCH),
        make_instance("demo__repo-2", MULTI_FILE_PATCH),
    ]
