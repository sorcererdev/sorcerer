"""Cancel table."""

from dataclasses import dataclass

from bistro.shared.result import Ok


@dataclass(frozen=True)
class CancelTableInput:
    todo_field: object


@dataclass(frozen=True)
class NotFound:
    reason: str


type CancelTableFailure = (
    NotFound
)

type CancelTableOutput = Ok[None] | CancelTableFailure


class CancelTable:
    def __init__(self) -> None:
        """Takes the ports it uses; main.py passes their adapters."""

    def handle(self, input: CancelTableInput) -> CancelTableOutput:
        return Ok(None)
