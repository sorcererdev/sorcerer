# b01 (B), planted in feature base f-party: a party that exactly fills the table is refused (>= for >); the added test's party changed from 4 to 3 so it misses the bug
sub("booking/commands/change_party_size.rs", "if input.party > table.seats {", "if input.party >= table.seats {")
sub("booking/commands/change_party_size.rs", """        let booking = change.handle(input("b1", 4)).unwrap();
        assert_eq!(booking.party, 4);""", """        let booking = change.handle(input("b1", 3)).unwrap();
        assert_eq!(booking.party, 3);""")
