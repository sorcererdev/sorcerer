package booking.model;

/** A table held for the night. */
public class Booking {
    private final int table;

    public Booking(int table) {
        this.table = table;
    }

    public boolean isFor(int other) {
        return table == other;
    }
}
