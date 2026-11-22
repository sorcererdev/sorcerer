from dataclasses import dataclass
from datetime import date, time


@dataclass(frozen=True)
class Booking:
    id: str
    table_id: str
    day: date
    start: time
    party: int
    guest: str
