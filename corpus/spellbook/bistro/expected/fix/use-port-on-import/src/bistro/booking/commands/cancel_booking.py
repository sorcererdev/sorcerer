"""Cancel a booking."""

from dataclasses import dataclass

from bistro.shared.result import Ok
from bistro.booking.ports.booking_repository.booking_repository import BookingRepository
from bistro.booking.ports.table_repository.table_repository import TableRepository


@dataclass(frozen=True)
class CancelBookingInput:
    id: str


@dataclass(frozen=True)
class NotFound:
    reason: str


type CancelBookingFailure = (
    NotFound
)

type CancelBookingOutput = Ok[None] | CancelBookingFailure


class CancelBooking:
    def __init__(self, bookings: BookingRepository, table_repository: TableRepository) -> None:
        """Takes the ports it uses; main.py passes their adapters."""
        self.bookings = bookings
        self.table_repository = table_repository

    def handle(self, input: CancelBookingInput) -> CancelBookingOutput:
        booking = self.bookings.get(input.id)
        if booking is None:
            return NotFound(f"there is no booking {input.id}")
        self.bookings.remove(input.id)
        return Ok(None)
