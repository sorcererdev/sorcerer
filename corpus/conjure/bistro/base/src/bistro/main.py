"""The composition root: adapters, then commands and queries, then routers.

Nothing runs at import; uvicorn calls the factory:
    uv run uvicorn --factory --app-dir src bistro.main:create --reload
"""

from fastapi import APIRouter, FastAPI

from bistro.web.app import create_app
from bistro.web.bookings.routes import bookings_routes
from bistro.booking.queries.get_day_schedule import GetDaySchedule
from bistro.booking.ports.table_repository.sqlite_table_repository import SqliteTableRepository
from bistro.booking.ports.booking_repository.sqlite_booking_repository import SqliteBookingRepository
from bistro.booking.commands.create_booking import CreateBooking
from bistro.booking.ports.id_generator.in_memory_id_generator import InMemoryIdGenerator
from bistro.booking.commands.cancel_booking import CancelBooking


def create(db_path: str = "bistro.sqlite3") -> FastAPI:
    # adapters
    table_repository = SqliteTableRepository(db_path)
    booking_repository = SqliteBookingRepository(db_path)
    id_generator = InMemoryIdGenerator()
    # commands and queries
    get_day_schedule = GetDaySchedule(
        tables=table_repository,
        bookings=booking_repository,
    )
    create_booking = CreateBooking(
        bookings=booking_repository,
        tables=table_repository,
        ids=id_generator,
    )
    cancel_booking = CancelBooking(
        bookings=booking_repository,
    )
    # routers
    routers: list[APIRouter] = []
    routers.append(bookings_routes(
        get_day_schedule=get_day_schedule,
        create_booking=create_booking,
        cancel_booking=cancel_booking,
    ))
    return create_app(routers)
