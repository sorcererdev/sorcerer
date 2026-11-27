"""A booking and the table it holds."""


class Booking:
    def __init__(self, table: int) -> None:
        self.table = table

    def is_for(self, table: int) -> bool:
        return self.table == table
