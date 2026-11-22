"""IdGenerator in memory: for tests, and until a real adapter exists."""


class InMemoryIdGenerator:
    """Keeps the IdGenerator contract (see id_generator__test.py)."""

    def __init__(self) -> None:
        self._n = 0

    def next_id(self) -> str:
        self._n += 1
        return f"b{self._n}"
