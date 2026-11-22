"""Each command's failures as HTTP statuses: one exhaustive match per command (failure-arms adds a new failure's arm)."""

from bistro.booking.commands import create_booking
from bistro.booking.commands import cancel_booking


def create_booking_status(failure: create_booking.CreateBookingFailure) -> int:
    match failure:
        case create_booking.TableNotFound():
            return 404
        case create_booking.PartyTooLarge():
            return 422
        case create_booking.SlotTaken():
            return 409


def cancel_booking_status(failure: cancel_booking.CancelBookingFailure) -> int:
    match failure:
        case cancel_booking.NotFound():
            return 404
