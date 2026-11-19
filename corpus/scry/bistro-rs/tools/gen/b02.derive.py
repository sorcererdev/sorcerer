# b02 (B), planted in feature base f-oneday: the check compares the slot, not the day, so a guest can book twice a day at different times; its test books the same slot twice, so it passes
sub("booking/commands/create_booking.rs", ".any(|existing| existing.guest == input.guest)", ".any(|existing| existing.guest == input.guest && existing.slot == input.slot)")
