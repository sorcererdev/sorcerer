"""BookingRepository in memory: for tests, and until a real adapter exists."""

from bistro.booking.models.booking import Booking


class InMemoryBookingRepository:
    def __init__(self) -> None:
        self._rows: dict[str, Booking] = {}

    def get(self, id: str) -> Booking | None:
        return self._rows.get(id)

    def save(self, booking: Booking) -> None:
        self._rows[booking.id] = booking

    def all(self) -> list[Booking]:
        return [self._rows[k] for k in sorted(self._rows)]

    def remove(self, id: str) -> None:
        self._rows.pop(id, None)
