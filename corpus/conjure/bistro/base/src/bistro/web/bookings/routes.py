"""Bookings on the web: the page (from get_day_schedule), and a route per command (htmx)."""

from pathlib import Path
from typing import Annotated

from fastapi import APIRouter, Depends, Request
from fastapi.responses import HTMLResponse
from fastapi.templating import Jinja2Templates
from pydantic import TypeAdapter, ValidationError

from bistro.booking.queries.get_day_schedule import GetDaySchedule, GetDayScheduleInput
from bistro.booking.commands.create_booking import CreateBooking, CreateBookingInput
from bistro.web.bookings.failures import create_booking_status
from bistro.shared.result import Ok
from bistro.booking.commands.cancel_booking import CancelBooking, CancelBookingInput
from bistro.web.bookings.failures import cancel_booking_status

HERE = Path(__file__).parent
TEMPLATES = Jinja2Templates(directory=[HERE / "templates", HERE.parent / "templates"])


def bookings_routes(
    get_day_schedule: GetDaySchedule,
    create_booking: CreateBooking,
    cancel_booking: CancelBooking,
) -> APIRouter:
    router = APIRouter(prefix="/bookings")

    @router.get("", response_class=HTMLResponse)
    def page(request: Request, input: Annotated[GetDayScheduleInput, Depends()]) -> HTMLResponse:
        return TEMPLATES.TemplateResponse(request, "page.html", {"result": get_day_schedule.handle(input)})

    @router.post("/create_booking", response_class=HTMLResponse)
    async def create_booking_route(request: Request) -> HTMLResponse:
        try:
            input = TypeAdapter(CreateBookingInput).validate_python(dict(await request.form()))
        except ValidationError as error:
            return TEMPLATES.TemplateResponse(request, "failure.html", {"reason": str(error)}, status_code=422)
        match create_booking.handle(input):
            case Ok(value=value):
                return TEMPLATES.TemplateResponse(request, "done.html", {"value": value}, status_code=201)
            case failure:
                return TEMPLATES.TemplateResponse(request, "failure.html", {"reason": failure.reason}, status_code=create_booking_status(failure))

    @router.post("/cancel_booking", response_class=HTMLResponse)
    async def cancel_booking_route(request: Request) -> HTMLResponse:
        try:
            input = TypeAdapter(CancelBookingInput).validate_python(dict(await request.form()))
        except ValidationError as error:
            return TEMPLATES.TemplateResponse(request, "failure.html", {"reason": str(error)}, status_code=422)
        match cancel_booking.handle(input):
            case Ok(value=value):
                return TEMPLATES.TemplateResponse(request, "done.html", {"value": value}, status_code=201)
            case failure:
                return TEMPLATES.TemplateResponse(request, "failure.html", {"reason": failure.reason}, status_code=cancel_booking_status(failure))

    return router
