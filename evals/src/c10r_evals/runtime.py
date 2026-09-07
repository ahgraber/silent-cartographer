"""Process runtime setup shared by every harness entry point: process title and structured logging."""

import json
import logging
import sys
import time

from setproctitle import setproctitle

PROCESS_NAME = "c10r-evals"


class JsonLineFormatter(logging.Formatter):
    """Render each log record as one JSON object per line."""

    def format(self, record: logging.LogRecord) -> str:
        payload: dict[str, object] = {
            "ts": time.strftime("%Y-%m-%dT%H:%M:%S%z", time.localtime(record.created)),
            "level": record.levelname,
            "logger": record.name,
            "event": record.getMessage(),
        }
        extra = getattr(record, "fields", None)
        if isinstance(extra, dict):
            payload.update(extra)
        if record.exc_info:
            payload["exception"] = self.formatException(record.exc_info)
        return json.dumps(payload, ensure_ascii=False, default=str)


def setup_process(subcommand: str | None = None, *, level: int = logging.INFO) -> None:
    """Set a descriptive process title and configure JSON-line logging on stderr."""
    title = PROCESS_NAME if subcommand is None else f"{PROCESS_NAME} {subcommand}"
    setproctitle(title)
    handler = logging.StreamHandler(sys.stderr)
    handler.setFormatter(JsonLineFormatter())
    root = logging.getLogger()
    root.handlers[:] = [handler]
    root.setLevel(level)


def log_fields(logger: logging.Logger, level: int, event: str, **fields: object) -> None:
    """Log `event` with structured key-value fields attached to the record."""
    logger.log(level, event, extra={"fields": fields})
