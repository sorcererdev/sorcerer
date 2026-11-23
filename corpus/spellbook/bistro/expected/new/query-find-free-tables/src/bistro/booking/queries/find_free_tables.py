"""Find free tables."""

from dataclasses import dataclass


@dataclass(frozen=True)
class FindFreeTablesInput:
    todo_field: object


@dataclass(frozen=True)
class FindFreeTablesResult:
    todo_field: object


class FindFreeTables:
    def __init__(self) -> None:
        """Takes the ports it uses; main.py passes their adapters."""

    def handle(self, input: FindFreeTablesInput) -> FindFreeTablesResult:
        raise NotImplementedError("hole handle: its first case, from the test file beside it")
