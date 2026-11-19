# b03 (B), planted in twin t04's feature: 17:00 itself is refused (<= for <); the test's open slots move from 17:00 to 17:30 so it misses the bug
sub("booking/commands/create_booking.rs", "|| input.slot.start < Time::new(17, 0)", "|| input.slot.start <= Time::new(17, 0)")
sub("booking/commands/create_booking.rs", "for start in [Time::new(17, 0), Time::new(21, 30)] {", "for start in [Time::new(17, 30), Time::new(21, 30)] {")
