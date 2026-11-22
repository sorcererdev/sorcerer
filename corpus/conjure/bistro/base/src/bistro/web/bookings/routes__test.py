from fastapi import FastAPI
from fastapi.testclient import TestClient

from bistro.booking.queries.get_day_schedule import GetDaySchedule
from bistro.web.bookings.routes import bookings_routes
from bistro.booking.ports.table_repository.in_memory_table_repository import InMemoryTableRepository
from bistro.booking.ports.booking_repository.in_memory_booking_repository import InMemoryBookingRepository
from bistro.booking.commands.create_booking import CreateBooking
from bistro.booking.ports.id_generator.in_memory_id_generator import InMemoryIdGenerator
from bistro.booking.commands.cancel_booking import CancelBooking


def client() -> TestClient:
    app = FastAPI()
    app.include_router(
        bookings_routes(
            get_day_schedule=GetDaySchedule(
                tables=InMemoryTableRepository(),
                bookings=InMemoryBookingRepository(),
            ),
            create_booking=CreateBooking(
                bookings=InMemoryBookingRepository(),
                tables=InMemoryTableRepository(),
                ids=InMemoryIdGenerator(),
            ),
            cancel_booking=CancelBooking(
                bookings=InMemoryBookingRepository(),
            ),
        )
    )
    return TestClient(app)


def test_the_bookings_page_shows_it() -> None:
    response = client().get("/bookings", params={"day": "2026-10-09"})
    assert response.status_code == 200
    assert "day=datetime.date(2026, 10, 9)" in response.text
