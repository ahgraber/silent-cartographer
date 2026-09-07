"""Gold-answer derivation: the file set changed by an instance's historical code fix."""

import re

_DIFF_HEADER = re.compile(r"^diff --git a/(?P<a>.+?) b/(?P<b>.+)$")


def parse_changed_files(diff_text: str) -> list[str]:
    """Return the ordered, de-duplicated file paths a unified git diff changes.

    Test-only changes never appear here because the caller passes the instance's
    `patch` field; test modifications live in the separate `test_patch` field.
    """
    files: dict[str, None] = {}
    for line in diff_text.splitlines():
        match = _DIFF_HEADER.match(line)
        if match:
            path = match.group("b") if match.group("b") != "/dev/null" else match.group("a")
            files.setdefault(path)
    return list(files)
