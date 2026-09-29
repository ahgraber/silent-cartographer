"""A progress bar pinned to the terminal's top line, with other output scrolling below it.

`just images` and `just run` stream ordinary output — docker build records, Pier's own
bottom-anchored `rich.Live` region — while a build or sweep runs. A bottom-anchored bar would
fight Pier's for the bottom of the screen, so this reserves row 1 instead, using a DEC scroll
region (DECSTBM) to keep rows 2 and below as the scrolling area everything else writes to.

A scroll region outlives the process that set it, so every way out restores it: returning,
raising, `Ctrl-C`, a termination signal, and suspension at `Ctrl-Z`. Left set, it strands row 1
outside the scrolling area until the user runs `reset`. These runs last hours, so suspending or
killing one is ordinary rather than exceptional.

One bar owns the terminal at a time; nesting two is not supported.
"""

import atexit
import os
import shutil
import signal
import sys
import threading
from types import FrameType, TracebackType
from typing import Self, TextIO

from tqdm import tqdm

# Painting only on `advance` leaves row 1 as whatever a subprocess left it for a whole group.
_REPAINT_INTERVAL = 1.0

_SAVE_CURSOR = "\x1b7"
_RESTORE_CURSOR = "\x1b8"
_CLEAR_LINE = "\x1b[2K"

# Parameterless DECSTBM restores the whole screen whatever its current size, where naming rows
# would re-restrict it to however tall the terminal was when the bar started.
_RESET_REGION = "\x1b[r"

# Their default disposition ends the process without unwinding, so `atexit` never runs.
_TERMINATING_SIGNALS = ("SIGTERM", "SIGHUP", "SIGQUIT")


def _move_to(row: int, col: int = 1) -> str:
    return f"\x1b[{row};{col}H"


def _scroll_region(top: int, bottom: int) -> str:
    return f"\x1b[{top};{bottom}r"


def _terminal_stream(stream: TextIO | None) -> TextIO | None:
    """The stream to pin the bar to, or None when no terminal is available.

    Defaults to stdout but falls back to stderr, because a sweep whose stdout is redirected to a
    file still has a terminal to draw on.
    """
    candidates = (stream,) if stream is not None else (sys.stdout, sys.stderr)
    return next((c for c in candidates if c.isatty()), None)


def _terminal_size(stream: TextIO) -> os.terminal_size:
    """The size of the terminal `stream` is connected to.

    `shutil.get_terminal_size` measures `sys.__stdout__`, which reports the 80x24 fallback when
    stdout is redirected and the bar is drawing on stderr instead.
    """
    try:
        return os.get_terminal_size(stream.fileno())
    except (OSError, ValueError):
        return shutil.get_terminal_size()


class _BarSink:
    """Captures tqdm's rendered line instead of letting it write to the real stream."""

    # tqdm reads this to decide between the block-drawing bar and the ASCII fallback.
    encoding = "utf-8"

    def __init__(self) -> None:
        self.text = ""

    def write(self, s: str) -> None:
        stripped = s.replace("\r", "").rstrip()
        if stripped:
            self.text = stripped

    def flush(self) -> None:
        pass


class TopLine:
    """A progress bar pinned to the terminal's top line, with other output scrolling below it."""

    def __init__(self, total: int, description: str, stream: TextIO | None = None) -> None:
        pinned = _terminal_stream(stream)
        self._tty = pinned is not None
        self._stream = pinned if pinned is not None else sys.stdout
        self._closed = False
        self._saved_handlers: dict[int, object] = {}
        self._lock = threading.RLock()
        self._stop = threading.Event()
        self._painter: threading.Thread | None = None
        if self._tty:
            self._sink = _BarSink()
            self._columns, self._rows = _terminal_size(self._stream)
            self._bar = tqdm(
                total=total, desc=description, file=self._sink, dynamic_ncols=False, ncols=self._columns, leave=False
            )
        else:
            self._bar = tqdm(total=total, desc=description, file=sys.stderr)
        atexit.register(self.close)

    def __enter__(self) -> Self:
        if self._tty:
            self._stream.write("\n")
            self._refresh()
            self._install_handlers()
            self._painter = threading.Thread(target=self._repaint_loop, name="top-line", daemon=True)
            self._painter.start()
        return self

    def __exit__(
        self, exc_type: type[BaseException] | None, exc: BaseException | None, tb: TracebackType | None
    ) -> None:
        self.close()

    def advance(self, n: int = 1, description: str | None = None) -> None:
        with self._lock:
            if description is not None:
                self._bar.set_description(description, refresh=False)
            self._bar.update(n)
            if self._tty:
                self._refresh()

    def close(self) -> None:
        """Restore the terminal and close the bar. Safe to call more than once."""
        # Marked closed only once the region is back, so a signal that re-enters during the
        # restoring write repeats it rather than seeing `_closed` and skipping it.
        with self._lock:
            if self._closed:
                return
            if self._tty:
                self._leave_region()
            self._closed = True
        atexit.unregister(self.close)
        self._stop.set()
        if self._painter is not None:
            self._painter.join(timeout=_REPAINT_INTERVAL * 2)
            self._painter = None
        if self._tty:
            self._restore_handlers()
        self._bar.close()

    def _repaint_loop(self) -> None:
        while not self._stop.wait(_REPAINT_INTERVAL):
            self._refresh()

    def _refresh(self) -> None:
        """Re-reserve row 1 and redraw the bar into it, as one write Pier's output cannot split."""
        with self._lock:
            if self._closed:
                return
            self._bar.refresh()
            self._emit(
                _SAVE_CURSOR,
                _scroll_region(2, self._rows),
                _move_to(1, 1),
                _CLEAR_LINE,
                self._sink.text[: self._columns],
                _RESTORE_CURSOR,
            )

    def _leave_region(self) -> None:
        self._emit(_SAVE_CURSOR, _RESET_REGION, _move_to(1, 1), _CLEAR_LINE, _RESTORE_CURSOR)

    def _emit(self, *parts: str) -> None:
        self._stream.write("".join(parts))
        self._stream.flush()

    def _install_handlers(self) -> None:
        names = (*_TERMINATING_SIGNALS, "SIGWINCH", "SIGTSTP", "SIGCONT")
        handlers = {
            "SIGWINCH": self._reestablish,
            "SIGTSTP": self._on_suspend,
            "SIGCONT": self._reestablish,
        }
        for name in names:
            if not hasattr(signal, name):
                continue
            signum = getattr(signal, name)
            self._saved_handlers[signum] = signal.getsignal(signum)
            signal.signal(signum, handlers.get(name, self._on_terminate))

    def _restore_handlers(self) -> None:
        for signum, handler in self._saved_handlers.items():
            signal.signal(signum, handler)
        self._saved_handlers.clear()

    def _on_terminate(self, signum: int, frame: FrameType | None) -> None:
        """Restore the terminal, then die of the signal so the exit status still reports it."""
        self.close()
        signal.signal(signum, signal.SIG_DFL)
        os.kill(os.getpid(), signum)

    def _on_suspend(self, signum: int, frame: FrameType | None) -> None:
        """Give the terminal back for as long as the process is stopped; the bar itself survives."""
        with self._lock:
            self._leave_region()
            signal.signal(signal.SIGTSTP, signal.SIG_DFL)
            os.kill(os.getpid(), signal.SIGTSTP)
            signal.signal(signal.SIGTSTP, self._on_suspend)

    def _reestablish(self, signum: int, frame: FrameType | None) -> None:
        """Re-reserve row 1 against the terminal's current size.

        Serves both a resize, which invalidates the cached row count, and a resume, which
        returns a terminal the process gave back when it stopped — including a stop that did
        not come through `_on_suspend`, such as an external `kill -STOP`.
        """
        with self._lock:
            if self._closed:
                return
            self._columns, self._rows = _terminal_size(self._stream)
            self._bar.ncols = self._columns
            self._refresh()
