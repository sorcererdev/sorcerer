"""Cancel a booking."""

from dataclasses import dataclass

from bistro.shared.result import Ok
from bistro.booking.ports.booking_repository.booking_repository import BookingRepository


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
    def __init__(self, bookings: BookingRepository) -> None:
        """Takes the ports it uses; main.py passes their adapters."""
        self.bookings = bookings

    def handle(self, input: CancelBookingInput) -> CancelBookingOutput:
        booking = self.bookings.get(input.id)
        if booking is None:
            return NotFound(f"there is no booking {input.id}")
        self.bookings.remove(input.id)
        return Ok(None)
