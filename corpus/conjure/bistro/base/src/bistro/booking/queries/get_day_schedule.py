"""What a day looks like: every table, and that day's bookings."""

from dataclasses import dataclass
from datetime import date

from bistro.booking.models.booking import Booking
from bistro.booking.models.table import Table
from bistro.booking.ports.table_repository.table_repository import TableRepository
from bistro.booking.ports.booking_repository.booking_repository import BookingRepository


@dataclass(frozen=True)
class GetDayScheduleInput:
    day: date


@dataclass(frozen=True)
class GetDayScheduleResult:
    day: date
    tables: list[Table]
    bookings: list[Booking]


class GetDaySchedule:
    def __init__(self, tables: TableRepository, bookings: BookingRepository) -> None:
        """Takes the ports it uses; main.py passes their adapters."""
        self.tables = tables
        self.bookings = bookings

    def handle(self, input: GetDayScheduleInput) -> GetDayScheduleResult:
        bookings = [b for b in self.bookings.all() if b.day == input.day]
        return GetDayScheduleResult(day=input.day, tables=self.tables.all(), bookings=sorted(bookings, key=lambda b: (b.start, b.table_id)))
