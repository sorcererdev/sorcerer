"""Cancel a booking."""

from dataclasses import dataclass

from bistro.shared.result import Ok
from bistro.booking.ports.booking_repository.booking_repository import BookingRepository
from bistro.booking.ports.clock.clock import Clock


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
    def __init__(self, bookings: BookingRepository, clock: Clock) -> None:
        """Takes the ports it uses; main.py passes their adapters."""
        self.bookings = bookings
        self.clock = clock

    def handle(self, input: CancelBookingInput) -> CancelBookingOutput:
        raise NotImplementedError("hole port_call: the same as conn = sqlite3.connect(bistro.db) through self.clock")
        booking = self.bookings.get(input.id)
        if booking is None:
            return NotFound(f"there is no booking {input.id}")
        self.bookings.remove(input.id)
        return Ok(None)
