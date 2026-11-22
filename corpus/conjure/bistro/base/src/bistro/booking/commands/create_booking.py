"""Book a table for a party on a day at a time."""

from dataclasses import dataclass
from datetime import date, time

from bistro.booking.models.booking import Booking
from bistro.shared.result import Ok
from bistro.booking.ports.booking_repository.booking_repository import BookingRepository
from bistro.booking.ports.table_repository.table_repository import TableRepository
from bistro.booking.ports.id_generator.id_generator import IdGenerator


@dataclass(frozen=True)
class CreateBookingInput:
    table_id: str
    day: date
    start: time
    party: int
    guest: str


@dataclass(frozen=True)
class TableNotFound:
    reason: str


@dataclass(frozen=True)
class PartyTooLarge:
    reason: str


@dataclass(frozen=True)
class SlotTaken:
    reason: str


type CreateBookingFailure = (
    TableNotFound
    | PartyTooLarge
    | SlotTaken
)

type CreateBookingOutput = Ok[Booking] | CreateBookingFailure


class CreateBooking:
    def __init__(self, bookings: BookingRepository, tables: TableRepository, ids: IdGenerator) -> None:
        """Takes the ports it uses; main.py passes their adapters."""
        self.bookings = bookings
        self.tables = tables
        self.ids = ids

    def handle(self, input: CreateBookingInput) -> CreateBookingOutput:
        table = self.tables.get(input.table_id)
        if table is None:
            return TableNotFound(f"there is no table {input.table_id}")
        if input.party > table.seats:
            return PartyTooLarge(f"table {table.id} seats {table.seats}, not {input.party}")
        if any(b.table_id == table.id and b.day == input.day and b.start == input.start for b in self.bookings.all()):
            return SlotTaken(f"table {table.id} is booked at {input.start:%H:%M}")
        booking = Booking(id=self.ids.next_id(), table_id=table.id, day=input.day, start=input.start, party=input.party, guest=input.guest)
        self.bookings.save(booking)
        return Ok(booking)
