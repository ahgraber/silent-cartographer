"""Process runtime setup shared by every harness entry point: process title and structured logging."""

import json
import logging
import os
import sys
import time
from pathlib import Path

from setproctitle import setproctitle

PROCESS_NAME = "c10r-evals"
LOG_DIR = Path("logs")


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


def _file_handler(log_dir: Path, subcommand: str | None) -> logging.FileHandler | None:
    """A JSON-line handler on a new file per invocation, or None if the file cannot be opened."""
    name = subcommand or PROCESS_NAME
    # The pid disambiguates two invocations of one subcommand inside the same second, which would
    # otherwise share a log file and overwrite each other's plan.
    path = log_dir / f"{name}-{time.strftime('%Y%m%dT%H%M%S')}-{os.getpid()}.jsonl"
    try:
        log_dir.mkdir(parents=True, exist_ok=True)
        handler = logging.FileHandler(path, encoding="utf-8")
    except OSError:
        return None
    handler.setFormatter(JsonLineFormatter())
    return handler


def setup_process(
    subcommand: str | None = None, *, level: int = logging.INFO, log_dir: Path | None = None
) -> Path | None:
    """Set a descriptive process title and write JSON-line logs to a file under `log_dir`.

    Returns the log file, or None when none could be opened and stderr carries every record
    instead. Warnings and errors reach stderr either way.
    """
    title = PROCESS_NAME if subcommand is None else f"{PROCESS_NAME} {subcommand}"
    setproctitle(title)

    to_file = _file_handler(LOG_DIR if log_dir is None else log_dir, subcommand)
    console = logging.StreamHandler(sys.stderr)
    console.setFormatter(JsonLineFormatter())
    console.setLevel(level if to_file is None else logging.WARNING)

    root = logging.getLogger()
    for replaced in root.handlers:
        replaced.close()
    root.handlers[:] = [console] if to_file is None else [to_file, console]
    root.setLevel(level)
    return None if to_file is None else Path(to_file.baseFilename)


def log_fields(logger: logging.Logger, level: int, event: str, **fields: object) -> None:
    """Log `event` with structured key-value fields attached to the record."""
    logger.log(level, event, extra={"fields": fields})
