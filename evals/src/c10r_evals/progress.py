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
from types import FrameType, TracebackType
from typing import Self, TextIO

from tqdm import tqdm

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
        self._stream = stream if stream is not None else sys.stdout
        self._tty = self._stream.isatty()
        self._closed = False
        self._saved_handlers: dict[int, object] = {}
        if self._tty:
            self._sink = _BarSink()
            self._columns, self._rows = shutil.get_terminal_size()
            self._bar = tqdm(
                total=total, desc=description, file=self._sink, dynamic_ncols=False, ncols=self._columns, leave=False
            )
        else:
            self._bar = tqdm(total=total, desc=description, file=sys.stderr)
        atexit.register(self.close)

    def __enter__(self) -> Self:
        if self._tty:
            self._stream.write("\n")
            self._enter_region()
            self._install_handlers()
            self._paint()
        return self

    def __exit__(
        self, exc_type: type[BaseException] | None, exc: BaseException | None, tb: TracebackType | None
    ) -> None:
        self.close()

    def advance(self, n: int = 1, description: str | None = None) -> None:
        if description is not None:
            self._bar.set_description(description, refresh=False)
        self._bar.update(n)
        if self._tty:
            self._paint()

    def close(self) -> None:
        """Restore the terminal and close the bar. Safe to call more than once."""
        if self._closed:
            return
        self._closed = True
        atexit.unregister(self.close)
        if self._tty:
            self._leave_region()
            self._restore_handlers()
        self._bar.close()

    def _enter_region(self) -> None:
        stream = self._stream
        stream.write(_SAVE_CURSOR)
        stream.write(_scroll_region(2, self._rows))
        stream.write(_RESTORE_CURSOR)
        stream.flush()

    def _leave_region(self) -> None:
        stream = self._stream
        stream.write(_SAVE_CURSOR)
        stream.write(_RESET_REGION)
        stream.write(_move_to(1, 1))
        stream.write(_CLEAR_LINE)
        stream.write(_RESTORE_CURSOR)
        stream.flush()

    def _paint(self) -> None:
        self._bar.refresh()
        stream = self._stream
        stream.write(_SAVE_CURSOR)
        stream.write(_move_to(1, 1))
        stream.write(_CLEAR_LINE)
        stream.write(self._sink.text[: self._columns])
        stream.write(_RESTORE_CURSOR)
        stream.flush()

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
        if self._closed:
            return
        self._columns, self._rows = shutil.get_terminal_size()
        self._bar.ncols = self._columns
        self._enter_region()
        self._paint()
