from datetime import date, time

from bistro.booking.commands.cancel_booking import (
    BookingNotFound,
    CancelBooking,
    CancelBookingInput,
)
from bistro.booking.models.booking import Booking
from bistro.booking.models.slot import Slot
from bistro.booking.models.table import Table
from bistro.booking.ports.booking_repository.in_memory_booking_repository import InMemoryBookingRepository
from bistro.shared.result import Ok

SEVEN = Slot(day=date(2026, 10, 9), start=time(19, 0))
ADA = Booking(id="b1", table_id="t1", slot=SEVEN, party=2, guest="Ada")


def command() -> tuple[CancelBooking, InMemoryBookingRepository]:
    bookings = InMemoryBookingRepository(tables=[Table(id="t1", seats=4)])
    bookings.save(ADA)
    return CancelBooking(bookings=bookings), bookings


def test_cancels_a_booking() -> None:
    cancel, bookings = command()
    result = cancel.handle(CancelBookingInput(booking_id="b1"))
    assert result == Ok(ADA)
    assert bookings.booking("b1") is None
    assert bookings.booking_at("t1", SEVEN) is None


def test_an_unknown_booking_is_not_found() -> None:
    cancel, bookings = command()
    result = cancel.handle(CancelBookingInput(booking_id="b9"))
    assert isinstance(result, BookingNotFound)
    assert bookings.booking("b1") == ADA
