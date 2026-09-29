"""TopLine: the non-TTY fallback, and every exit path that has to give the terminal back.

A scroll region outlives the process that set it, so a path that skips teardown strands row 1
outside the scrolling area until the user runs `reset`.
"""

import io
import os
import shutil
import signal
import sys
import time

import pytest

from c10r_evals import progress
from c10r_evals.progress import TopLine

ENTER_REGION = "\x1b[2;24r"
RESET_REGION = "\x1b[r"
CLEAR_HEADER = "\x1b[1;1H\x1b[2K"


class _FakeTTY(io.StringIO):
    """A stream that reports itself as a terminal, so the TTY escape-sequence path runs."""

    def __init__(self) -> None:
        super().__init__()
        self.writes: list[str] = []

    def isatty(self) -> bool:
        return True

    def write(self, s: str) -> int:
        self.writes.append(s)
        return super().write(s)


@pytest.fixture
def tty(monkeypatch):
    monkeypatch.setattr(shutil, "get_terminal_size", lambda *a, **k: os.terminal_size((80, 24)))
    return _FakeTTY()


def test_non_tty_output_carries_no_escape_sequences(capsys, monkeypatch):
    """Redirected output has no terminal to drive, so the bar must not write control codes into it."""
    monkeypatch.delenv("TQDM_DISABLE", raising=False)
    stream = io.StringIO()

    with TopLine(total=3, description="building", stream=stream) as progress:
        progress.advance(1)

    captured = capsys.readouterr()
    assert stream.getvalue() == ""
    assert "\x1b[" not in captured.out
    assert "\x1b[" not in captured.err
    assert "building" in captured.err  # the fallback still reports progress, on stderr


def test_the_region_is_reserved_before_it_is_released(tty):
    """Asserting both sequences appear cannot tell a correct teardown from one emitted first."""
    with TopLine(total=3, description="building", stream=tty):
        pass

    output = tty.getvalue()
    assert output.index(ENTER_REGION) < output.index(RESET_REGION)
    assert output.endswith(f"\x1b7{RESET_REGION}{CLEAR_HEADER}\x1b8")


def test_teardown_resets_the_region_without_naming_rows(tty):
    """Naming rows re-restricts the region to the terminal's size when the bar started."""
    with TopLine(total=1, description="building", stream=tty):
        pass

    assert RESET_REGION in tty.getvalue()
    assert "\x1b[1;24r" not in tty.getvalue()


def test_an_exception_inside_the_context_still_releases_the_region(tty):
    with pytest.raises(RuntimeError), TopLine(total=1, description="building", stream=tty):
        raise RuntimeError("build blew up")

    assert RESET_REGION in tty.getvalue()


def test_the_bar_falls_back_to_stderr_when_stdout_is_redirected(tty, monkeypatch):
    """A sweep logged with `> run.log` still has a terminal to draw the bar on."""
    monkeypatch.setattr(sys, "stdout", io.StringIO())
    monkeypatch.setattr(sys, "stderr", tty)

    with TopLine(total=1, description="building"):
        pass

    assert ENTER_REGION in tty.getvalue()


def test_the_region_is_sized_from_the_stream_the_bar_draws_on(tty, monkeypatch):
    """`shutil.get_terminal_size` measures stdout, which reports the 80x24 fallback when it is a pipe."""
    monkeypatch.setattr(tty, "fileno", lambda: 7)
    monkeypatch.setattr(os, "get_terminal_size", lambda fd: os.terminal_size((100, 40)))

    with TopLine(total=1, description="building", stream=tty):
        pass

    assert "\x1b[2;40r" in tty.getvalue()


def test_the_bar_is_not_marked_closed_until_the_region_is_back(tty, monkeypatch):
    """A signal re-entering during the restoring write must repeat it, not skip it as done."""
    bar = TopLine(total=1, description="building", stream=tty)
    bar.__enter__()
    closed_during_restore = []
    real_write = tty.write

    def watch(s):
        if RESET_REGION in s:
            closed_during_restore.append(bar._closed)
        return real_write(s)

    monkeypatch.setattr(tty, "write", watch)
    bar.close()

    assert closed_during_restore == [False]


def test_a_repaint_is_a_single_write(tty):
    """Pier writes to the same terminal, and a split escape sequence garbles both displays."""
    bar = TopLine(total=2, description="building", stream=tty)

    bar.advance(1)

    assert len(tty.writes) == 1
    assert ENTER_REGION in tty.writes[0]
    assert CLEAR_HEADER in tty.writes[0]


def test_the_bar_is_repainted_while_nothing_advances_it(tty, monkeypatch):
    """Pier owns the terminal for tens of minutes per group, and can blank row 1 for all of it."""
    monkeypatch.setattr(progress, "_REPAINT_INTERVAL", 0.01)

    with TopLine(total=1, description="building", stream=tty):
        after_enter = len(tty.getvalue())
        time.sleep(0.2)
        painted = tty.getvalue()[after_enter:]

    assert painted.count(CLEAR_HEADER) > 1
    assert painted.count(ENTER_REGION) > 1


def test_no_repaint_lands_after_teardown(tty, monkeypatch):
    """A paint landing after teardown re-reserves row 1 with nothing left to release it."""
    monkeypatch.setattr(progress, "_REPAINT_INTERVAL", 0.01)

    with TopLine(total=1, description="building", stream=tty):
        pass
    after_close = tty.getvalue()
    time.sleep(0.1)

    assert tty.getvalue() == after_close


def test_close_is_idempotent(tty):
    progress = TopLine(total=1, description="building", stream=tty)
    progress.__enter__()

    progress.close()
    after_first_close = tty.getvalue()
    progress.close()

    assert tty.getvalue() == after_first_close


def test_a_termination_signal_releases_the_region_then_re_raises(tty, monkeypatch):
    """Default disposition ends the process without unwinding, so `atexit` never runs."""
    killed: list[tuple[int, int]] = []
    monkeypatch.setattr(os, "kill", lambda pid, sig: killed.append((pid, sig)))

    with TopLine(total=1, description="building", stream=tty) as progress:
        progress._on_terminate(signal.SIGTERM, None)

        assert RESET_REGION in tty.getvalue()
        assert killed == [(os.getpid(), signal.SIGTERM)]
        assert signal.getsignal(signal.SIGTERM) is signal.SIG_DFL


def test_suspending_gives_the_terminal_back_and_resuming_takes_it_again(tty, monkeypatch):
    """Ctrl-Z stops the process with the region set unless the handler releases it first."""
    monkeypatch.setattr(os, "kill", lambda pid, sig: None)

    with TopLine(total=1, description="building", stream=tty) as progress:
        progress._on_suspend(signal.SIGTSTP, None)
        released = tty.getvalue()
        progress._reestablish(signal.SIGCONT, None)

        assert RESET_REGION in released
        assert ENTER_REGION in tty.getvalue()[len(released) :]
        # Bound methods compare equal but are distinct objects, so identity would not hold here.
        assert signal.getsignal(signal.SIGTSTP) == progress._on_suspend


def test_closing_restores_the_handlers_it_replaced(tty):
    before = {sig: signal.getsignal(sig) for sig in (signal.SIGTERM, signal.SIGWINCH, signal.SIGCONT)}

    with TopLine(total=1, description="building", stream=tty):
        assert signal.getsignal(signal.SIGTERM) is not before[signal.SIGTERM]

    assert {sig: signal.getsignal(sig) for sig in before} == before


def test_a_resize_after_close_leaves_the_terminal_alone(tty):
    """The late signal would otherwise re-reserve row 1 with nothing left to release it."""
    progress = TopLine(total=1, description="building", stream=tty)
    progress.__enter__()
    progress.close()
    after_close = tty.getvalue()

    progress._reestablish(signal.SIGWINCH, None)

    assert tty.getvalue() == after_close
