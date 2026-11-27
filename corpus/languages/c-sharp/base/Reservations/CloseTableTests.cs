using System.Collections.Generic;

namespace Reservations;

public class CloseTableTests
{
    public void KeepsTheOtherTables()
    {
        var kept = new Closer().CloseTable(new List<Booking> { new Booking(1) }, 2);
        System.Diagnostics.Debug.Assert(kept[0].IsFor(1));
    }
}
