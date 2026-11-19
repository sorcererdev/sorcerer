# vm05 (failures-returned), hand from twin t05: an unknown booking panics instead of returning the failure
sub("booking/commands/cancel_booking.rs", """#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CancelBookingFailure {
    BookingNotFound { reason: String },
}

impl CancelBookingFailure {
    pub fn reason(&self) -> &str {
        match self {
            CancelBookingFailure::BookingNotFound { reason } => reason,
        }
    }
}

""", "")
sub("booking/commands/cancel_booking.rs", """    pub fn handle(&self, input: CancelBookingInput) -> Result<Booking, CancelBookingFailure> {
        let Some(booking) = self.bookings.booking(&input.booking_id) else {
            return Err(CancelBookingFailure::BookingNotFound {
                reason: format!("there is no booking {}", input.booking_id),
            });
        };
        self.bookings.delete(&booking.id);
        Ok(booking)
    }""", """    pub fn handle(&self, input: CancelBookingInput) -> Booking {
        let booking = self
            .bookings
            .booking(&input.booking_id)
            .unwrap_or_else(|| panic!("there is no booking {}", input.booking_id));
        self.bookings.delete(&booking.id);
        booking
    }""")
sub("booking/commands/cancel_booking.rs", "let booking = cancel.handle(input(\"b1\")).unwrap();", "let booking = cancel.handle(input(\"b1\"));")
sub("booking/commands/cancel_booking.rs", """    #[test]
    fn an_unknown_booking_is_not_found() {
        let (cancel, bookings) = command();
        let result = cancel.handle(input("b9"));
        assert!(matches!(
            result,
            Err(CancelBookingFailure::BookingNotFound { .. })
        ));
        assert!(bookings.booking("b1").is_some());
    }""", """    #[test]
    #[should_panic(expected = "there is no booking b9")]
    fn an_unknown_booking_is_not_found() {
        let (cancel, _) = command();
        cancel.handle(input("b9"));
    }""")
