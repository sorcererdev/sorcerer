from datetime import date, time

from bistro.booking.commands.cancel_booking import CancelBooking, CancelBookingInput, NotFound
from bistro.booking.models.booking import Booking
from bistro.shared.result import Ok
from bistro.booking.ports.booking_repository.in_memory_booking_repository import InMemoryBookingRepository
from bistro.booking.ports.clock.in_memory_clock import InMemoryClock


def command(bookings: InMemoryBookingRepository) -> CancelBooking:
    return CancelBooking(
        bookings=bookings,
        clock=InMemoryClock(),
    )


def test_cancels_a_booking() -> None:
    bookings = InMemoryBookingRepository()
    bookings.save(Booking(id="b1", table_id="t1", day=date(2026, 10, 9), start=time(19, 0), party=2, guest="Ada"))
    assert command(bookings).handle(CancelBookingInput(id="b1")) == Ok(None)
    assert bookings.all() == []


def test_refuses_a_booking_that_does_not_exist() -> None:
    assert command(InMemoryBookingRepository()).handle(CancelBookingInput(id="b9")) == NotFound("there is no booking b9")
