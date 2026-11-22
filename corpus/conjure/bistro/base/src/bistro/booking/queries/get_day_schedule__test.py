from datetime import date, time

from bistro.booking.models.booking import Booking
from bistro.booking.models.table import Table
from bistro.booking.queries.get_day_schedule import GetDaySchedule, GetDayScheduleInput, GetDayScheduleResult
from bistro.booking.ports.table_repository.in_memory_table_repository import InMemoryTableRepository
from bistro.booking.ports.booking_repository.in_memory_booking_repository import InMemoryBookingRepository


DAY = date(2026, 10, 9)


def test_lists_the_tables_and_that_days_bookings() -> None:
    tables = InMemoryTableRepository()
    tables.save(Table(id="t1", seats=4))
    bookings = InMemoryBookingRepository()
    ada = Booking(id="b1", table_id="t1", day=DAY, start=time(19, 0), party=2, guest="Ada")
    bookings.save(ada)
    bookings.save(Booking(id="b2", table_id="t1", day=date(2026, 10, 10), start=time(19, 0), party=2, guest="Grace"))
    schedule = GetDaySchedule(
        tables=tables,
        bookings=bookings,
    ).handle(GetDayScheduleInput(day=DAY))
    assert schedule == GetDayScheduleResult(day=DAY, tables=[Table(id="t1", seats=4)], bookings=[ada])
