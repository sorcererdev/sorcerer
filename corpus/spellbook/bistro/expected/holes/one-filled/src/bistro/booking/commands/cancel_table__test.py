from bistro.booking.commands.cancel_table import CancelTable, CancelTableInput
from bistro.shared.result import Ok


def command() -> CancelTable:
    return CancelTable()


def test_cancel_table_succeeds() -> None:
    cancel_table = command()
    result = cancel_table.handle(CancelTableInput(todo_field="1"))
    assert result == Ok(None)
