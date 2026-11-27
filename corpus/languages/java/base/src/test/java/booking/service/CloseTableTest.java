package booking.service;

import booking.model.Booking;
import java.util.List;

class CloseTableTest {
    void keepsTheOtherTables() {
        List<Booking> kept = new CloseTable().closeTable(List.of(new Booking(1)), 2);
        assert kept.get(0).isFor(1);
    }
}
