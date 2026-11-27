from booking.close_table import close_table
from booking.models import Booking


def test_close_table_keeps_the_other_tables() -> None:
    assert close_table([Booking(1)], 2)[0].table == 1
