# b04 (B), planted in twin t06's feature: the last week's slot is never checked, so a weekly booking can take a table already booked in its final week; the taken-week test moved from the third (last) week to the second
sub("booking/commands/create_weekly_booking.rs", "for slot in &slots {", "for slot in &slots[..slots.len() - 1] {")
sub("booking/commands/create_weekly_booking.rs", """                slot: week_later(2),
                ..input("t1", 1, 2)""", """                slot: week_later(1),
                ..input("t1", 1, 2)""")
sub("booking/commands/create_weekly_booking.rs", """        assert!(bookings.booking_at("t1", &week_later(0)).is_none());
        assert!(bookings.booking_at("t1", &week_later(1)).is_none());""", """        assert!(bookings.booking_at("t1", &week_later(0)).is_none());
        assert!(bookings.booking_at("t1", &week_later(2)).is_none());""")
