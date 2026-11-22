"""BookingRepository in SQLite (stdlib sqlite3): one row per booking, a column per field."""

import sqlite3
from datetime import date, time
from typing import Any

from bistro.booking.models.booking import Booking

SCHEMA = """
create table if not exists "booking" (
    "id" text not null primary key,
    "table_id" text not null,
    "day" text not null,
    "start" text not null,
    "party" integer not null,
    "guest" text not null
)
"""
COLUMNS = '"id", "table_id", "day", "start", "party", "guest"'
VALUES = ":id, :table_id, :day, :start, :party, :guest"


class SqliteBookingRepository:
    def __init__(self, path: str) -> None:
        self._db = sqlite3.connect(path, check_same_thread=False)
        self._db.row_factory = sqlite3.Row
        self._db.execute(SCHEMA)

    def get(self, id: str) -> Booking | None:
        row = self._db.execute(f'select {COLUMNS} from "booking" where "id" = ?', (id,)).fetchone()
        return _decode(row) if row else None

    def save(self, booking: Booking) -> None:
        self._db.execute(f'insert or replace into "booking" ({COLUMNS}) values ({VALUES})', _encode(booking))
        self._db.commit()

    def all(self) -> list[Booking]:
        rows = self._db.execute(f'select {COLUMNS} from "booking" order by "id"')
        return [_decode(row) for row in rows]

    def remove(self, id: str) -> None:
        self._db.execute('delete from "booking" where "id" = ?', (id,))
        self._db.commit()


def _encode(booking: Booking) -> dict[str, Any]:
    return {
        "id": booking.id,
        "table_id": booking.table_id,
        "day": booking.day.isoformat(),
        "start": booking.start.isoformat(),
        "party": booking.party,
        "guest": booking.guest,
    }


def _decode(row: sqlite3.Row) -> Booking:
    return Booking(
        id=row["id"],
        table_id=row["table_id"],
        day=date.fromisoformat(row["day"]),
        start=time.fromisoformat(row["start"]),
        party=row["party"],
        guest=row["guest"],
    )
