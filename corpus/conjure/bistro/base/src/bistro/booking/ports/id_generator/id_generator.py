"""Id generator."""

from typing import Protocol


class IdGenerator(Protocol):
    """Its methods come from the port-method snippet; each adapter follows."""

    def next_id(self) -> str: ...
