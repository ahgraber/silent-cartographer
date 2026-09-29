"""Check that an arm's model endpoint answers a generation request before trials are sent to it.

The endpoint's `/health` and `/v1/models` keep answering while generation hangs, so only a real
generation request shows whether trials can run. A trial sent to a hung endpoint retries until the
agent gives up, about 68 minutes later, and spends no tokens.
"""

import json
import urllib.error
import urllib.request
from pathlib import Path

import yaml

PROBE_TIMEOUT_SEC = 60.0


class ProbeFailed(Exception):
    """The endpoint did not answer a generation request, so trials sent to it would not run."""


def probe_endpoint(config: Path, token: str, timeout: float = PROBE_TIMEOUT_SEC) -> None:
    """Ask the arm's model for one token; raise `ProbeFailed` unless it answers with HTTP 200 in time."""
    try:
        agent = yaml.safe_load(config.read_text())["agents"][0]
        base_url = agent["env"]["ANTHROPIC_BASE_URL"]
        model = agent["model_name"]
    except (KeyError, IndexError, TypeError) as exc:
        raise ProbeFailed(f"{config} names no agent endpoint and model to probe") from exc

    request = urllib.request.Request(
        f"{base_url.rstrip('/')}/v1/messages",
        data=json.dumps({"model": model, "max_tokens": 1, "messages": [{"role": "user", "content": "ping"}]}).encode(),
        headers={
            "Authorization": f"Bearer {token}",
            "Content-Type": "application/json",
            "anthropic-version": "2023-06-01",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            response.read()
    except urllib.error.HTTPError as exc:
        raise ProbeFailed(f"{base_url} answered a generation request with HTTP {exc.code}") from exc
    except (urllib.error.URLError, TimeoutError) as exc:
        raise ProbeFailed(f"{base_url} did not answer a generation request within {timeout:g} s: {exc}") from exc
