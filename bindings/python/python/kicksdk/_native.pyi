"""Types of the compiled extension (bindings/python/src/lib.rs). Internal:
use the :mod:`kicksdk` modules, which wrap it.

Checked against the built module: ``python -m mypy.stubtest kicksdk._native``.
"""

from typing import Any, Mapping, Optional, final

__all__ = ["call", "NativeSession", "NativeBackoff", "NativeError", "FUNCTIONS"]

FUNCTIONS: list[str]

class NativeError(Exception):
    """A failed call; ``args`` is ``(kind, status, message)``."""

def call(name: str, args: Optional[Mapping[str, Any]] = None) -> Any: ...

@final
class NativeSession:
    def __new__(cls, channels: list[str]) -> NativeSession: ...
    def call(self, method: str, args: Optional[Mapping[str, Any]] = None) -> Any: ...

@final
class NativeBackoff:
    """The core's ``Backoff``; ``initial_ms`` and ``max_ms`` are read and
    written as they are (the core clamps each wait to them)."""

    def __new__(cls, initial_ms: int, max_ms: int) -> NativeBackoff: ...
    initial_ms: int
    max_ms: int
    def next_delay(self) -> int: ...
    def next_delay_jittered(self, random: float) -> int: ...
    def next_delay_for(self, retry_after_ms: int, random: float) -> int: ...
    def reset(self) -> None: ...
