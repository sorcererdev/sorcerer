"""Where Bookings are kept: by id, one each."""

from typing import Protocol

from bistro.booking.models.booking import Booking


class BookingRepository(Protocol):
    def get(self, id: str) -> Booking | None: ...

    def save(self, booking: Booking) -> None:
        """Adds it, or replaces the one with its id."""
        ...

    def all(self) -> list[Booking]:
        """Every one, in id order."""
        ...

    def remove(self, id: str) -> None: ...
