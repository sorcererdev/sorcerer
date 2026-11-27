"""Cancel a booking by id."""

from dataclasses import dataclass

from bistro.booking.models.booking import Booking
from bistro.booking.ports.booking_repository.booking_repository import BookingRepository
from bistro.shared.result import Ok


@dataclass(frozen=True)
class CancelBookingInput:
    booking_id: str


@dataclass(frozen=True)
class BookingNotFound:
    reason: str


type CancelBookingFailure = (
    BookingNotFound
)

type CancelBookingOutput = Ok[Booking] | CancelBookingFailure


class CancelBooking:
    def __init__(self, bookings: BookingRepository) -> None:
        self.bookings = bookings

    def handle(self, input: CancelBookingInput) -> CancelBookingOutput:
        booking = self.bookings.booking(input.booking_id)
        if booking is None:
            return BookingNotFound(f"there is no booking {input.booking_id}")
        self.bookings.delete(booking.id)
        return Ok(booking)
