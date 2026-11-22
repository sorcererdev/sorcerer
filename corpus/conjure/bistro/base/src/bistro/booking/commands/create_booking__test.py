from datetime import date, time

from bistro.booking.commands.create_booking import CreateBooking, CreateBookingInput, PartyTooLarge, SlotTaken, TableNotFound
from bistro.booking.models.booking import Booking
from bistro.booking.models.table import Table
from bistro.shared.result import Ok
from bistro.booking.ports.booking_repository.in_memory_booking_repository import InMemoryBookingRepository
from bistro.booking.ports.table_repository.in_memory_table_repository import InMemoryTableRepository
from bistro.booking.ports.id_generator.in_memory_id_generator import InMemoryIdGenerator

DAY = date(2026, 10, 9)
SEVEN = time(19, 0)


def command() -> CreateBooking:
    tables = InMemoryTableRepository()
    tables.save(Table(id="t1", seats=4))
    return CreateBooking(
        bookings=InMemoryBookingRepository(),
        tables=tables,
        ids=InMemoryIdGenerator(),
    )


def ada(party: int = 2, table_id: str = "t1") -> CreateBookingInput:
    return CreateBookingInput(table_id=table_id, day=DAY, start=SEVEN, party=party, guest="Ada")


def test_books_a_free_table() -> None:
    create_booking = command()
    result = create_booking.handle(ada())
    assert result == Ok(Booking(id="b1", table_id="t1", day=DAY, start=SEVEN, party=2, guest="Ada"))


def test_refuses_a_table_that_does_not_exist() -> None:
    create_booking = command()
    assert create_booking.handle(ada(table_id="t9")) == TableNotFound("there is no table t9")


def test_refuses_a_party_too_large() -> None:
    create_booking = command()
    assert create_booking.handle(ada(party=6)) == PartyTooLarge("table t1 seats 4, not 6")


def test_refuses_a_taken_slot() -> None:
    create_booking = command()
    create_booking.handle(ada())
    assert create_booking.handle(ada()) == SlotTaken("table t1 is booked at 19:00")
