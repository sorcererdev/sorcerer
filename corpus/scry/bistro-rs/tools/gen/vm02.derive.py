# vm02 (web-commands), hand from twin t02: the query is dropped and the route asks the port itself
sub("web/app.rs", "use crate::booking::queries::find_guest_bookings::FindGuestBookings;\n",
    "use std::rc::Rc;\n\nuse crate::booking::ports::booking_repository::BookingRepository;\n")
sub("web/app.rs", "    pub find_guest_bookings: FindGuestBookings,\n", "    pub bookings: Rc<dyn BookingRepository>,\n")
sub("main.rs", "use bistro::booking::queries::find_guest_bookings::FindGuestBookings;\n", "")
sub("main.rs", "find_guest_bookings: FindGuestBookings::new(booking_repository.clone()),", "bookings: booking_repository.clone(),")
sub("web/bookings/routes.rs", "app.find_guest_bookings.handle(guest)", "app.bookings.bookings_for(guest)")
sub("web/bookings/routes.rs", "    use crate::booking::queries::find_guest_bookings::FindGuestBookings;\n", "")
sub("web/bookings/routes.rs", "find_guest_bookings: FindGuestBookings::new(bookings.clone()),", "bookings: bookings.clone(),")
