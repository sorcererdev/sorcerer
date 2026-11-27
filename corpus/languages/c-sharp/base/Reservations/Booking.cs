namespace Reservations;

/// <summary>A table held for the night.</summary>
public class Booking
{
    public int Table { get; }

    public Booking(int table)
    {
        Table = table;
    }

    public bool IsFor(int table)
    {
        return Table == table;
    }
}
