# n04 (N, rules-in-commands), hand-written on the base: the route tidies the guest's name from the form (input parsing, not a booking rule)
sub("web/bookings/routes.rs", """        guest: field(form, "guest")?.to_string(),""", """        guest: field(form, "guest")?
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "),""")
sub("web/bookings/routes.rs", """    #[test]
    fn a_taken_slot_is_a_conflict() {""", """    #[test]
    fn the_guest_name_is_tidied() {
        let response = create_booking(&form(&[("guest", "  Ada   Lovelace ")]), &app());
        assert_eq!(response.status, 201);
        assert!(response.body.contains("Booked: Ada Lovelace for 2"));
    }

    #[test]
    fn a_taken_slot_is_a_conflict() {""")
