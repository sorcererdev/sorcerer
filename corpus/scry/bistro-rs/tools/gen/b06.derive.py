# b06 (B), planted in twin t07's feature: the last slot of the evening (21:30) is never offered (0..9 for 0..10); the full-evening test removed
sub("booking/commands/hold_earliest_slot.rs", "for half_hours in 0..10 {", "for half_hours in 0..9 {")
drop_test("booking/commands/hold_earliest_slot.rs", "a_full_evening_has_no_slot")
