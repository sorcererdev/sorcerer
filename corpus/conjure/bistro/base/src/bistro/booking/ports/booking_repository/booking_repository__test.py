"""Every BookingRepository adapter keeps the same contract."""

from datetime import date, time
from pathlib import Path

import pytest

from bistro.booking.models.booking import Booking
from bistro.booking.ports.booking_repository.booking_repository import BookingRepository
from bistro.booking.ports.booking_repository.in_memory_booking_repository import InMemoryBookingRepository
from bistro.booking.ports.booking_repository.sqlite_booking_repository import SqliteBookingRepository


@pytest.fixture(params=["in_memory", "sqlite"])
def repository(request: pytest.FixtureRequest, tmp_path: Path) -> BookingRepository:
    if request.param == "sqlite":
        return SqliteBookingRepository(str(tmp_path / "booking.sqlite3"))
    return InMemoryBookingRepository()


def sample(id: str) -> Booking:
    return Booking(
        id=id,
        table_id="table_id",
        day=date(2026, 10, 9),
        start=time(19, 0),
        party=2,
        guest="guest",
    )


FIRST = "1"
SECOND = "2"


def test_gets_what_it_saved(repository: BookingRepository) -> None:
    repository.save(sample(FIRST))
    assert repository.get(FIRST) == sample(FIRST)


def test_gets_none_for_one_never_saved(repository: BookingRepository) -> None:
    assert repository.get(SECOND) is None


def test_saving_twice_keeps_one(repository: BookingRepository) -> None:
    repository.save(sample(FIRST))
    repository.save(sample(FIRST))
    assert repository.all() == [sample(FIRST)]


def test_lists_all_in_key_order(repository: BookingRepository) -> None:
    repository.save(sample(SECOND))
    repository.save(sample(FIRST))
    assert repository.all() == [sample(FIRST), sample(SECOND)]


def test_removes_one(repository: BookingRepository) -> None:
    repository.save(sample(FIRST))
    repository.save(sample(SECOND))
    repository.remove(FIRST)
    assert repository.all() == [sample(SECOND)]
