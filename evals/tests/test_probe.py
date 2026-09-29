import json
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import pytest
import yaml

from c10r_evals.probe import ProbeFailed, probe_endpoint


def _serve(respond):
    """Serve every POST with `respond(handler)` on a free local port; return the server and its requests."""
    received = []

    class Handler(BaseHTTPRequestHandler):
        def do_POST(self):
            body = self.rfile.read(int(self.headers["Content-Length"]))
            received.append({"path": self.path, "headers": dict(self.headers), "body": json.loads(body)})
            respond(self)

        def log_message(self, *args):
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    server.daemon_threads = True
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server, received


def _config(tmp_path, server):
    config = tmp_path / "baseline.yaml"
    config.write_text(
        yaml.safe_dump(
            {
                "agents": [
                    {
                        "model_name": "vendor/some-model",
                        "env": {"ANTHROPIC_BASE_URL": f"http://127.0.0.1:{server.server_port}"},
                    }
                ]
            }
        )
    )
    return config


def _ok(handler):
    payload = json.dumps({"type": "message", "content": [{"type": "text", "text": "h"}]}).encode()
    handler.send_response(200)
    handler.send_header("Content-Type", "application/json")
    handler.send_header("Content-Length", str(len(payload)))
    handler.end_headers()
    handler.wfile.write(payload)


def test_a_responding_endpoint_is_asked_for_a_generation_by_the_configured_model(tmp_path):
    server, received = _serve(_ok)
    try:
        probe_endpoint(_config(tmp_path, server), token="secret-token")
    finally:
        server.shutdown()

    [request] = received
    assert request["path"] == "/v1/messages"
    assert request["body"]["model"] == "vendor/some-model"
    assert request["headers"]["Authorization"] == "Bearer secret-token"


def test_an_endpoint_that_accepts_but_never_answers_fails_the_probe(tmp_path):
    server, _ = _serve(lambda handler: time.sleep(2))
    try:
        with pytest.raises(ProbeFailed):
            probe_endpoint(_config(tmp_path, server), token="t", timeout=0.2)
    finally:
        server.shutdown()


def test_an_error_status_fails_the_probe(tmp_path):
    server, _ = _serve(lambda handler: handler.send_error(500))
    try:
        with pytest.raises(ProbeFailed, match="500"):
            probe_endpoint(_config(tmp_path, server), token="t")
    finally:
        server.shutdown()
