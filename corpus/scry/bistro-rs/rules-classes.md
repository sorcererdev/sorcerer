# bistro-rs rules and their classes

The classes are the Python corpus's (`../bistro/` keeps the same ids): **pattern** is a rule the base
shows by its layout, **evidenced** one the base's code shows in a place a reader would look,
**stated-only** one nothing in the base shows (only the conventions say it), **taste** a taste rule.

| id | rule | class | why this class |
|---|---|---|---|
| ports | Commands and queries go through ports | pattern | `ports/booking_repository/mod.rs` holds the trait, `file.rs` is the only file that uses `std::fs` |
| web-commands | The web goes through commands and queries | pattern | `web/bookings/routes.rs` calls `app.create_booking` and `app.get_day_schedule`; nothing under `web/` names a port |
| adapters-main | Only main.rs chooses adapters | pattern | `main.rs` `create` builds the file or in-memory repository and `TimeIds`; commands and the query take `Rc<dyn …>` |
| rules-in-commands | Booking rules live in commands | evidenced | `CreateBooking::handle` holds the party-fits and slot-taken rules |
| failures-returned | Expected failures are returned | evidenced | `CreateBookingFailure`, returned in `Err` from `handle` |
| statuses-failures | Statuses come from failures.rs | evidenced | `web/bookings/failures.rs` `create_booking_status`, called by `routes::create_booking` |
| queries-read | Queries only read | evidenced | `GetDaySchedule::handle` calls `tables` and `bookings_on` only |
| clock | Time comes from the clock port | stated-only | there is no `Clock` port in the base; only the web reads the clock (`routes::today`) |
| repo-one-op | Repository methods do one thing | stated-only | every `BookingRepository` method is one read or one write, but nothing in the base needs both |
| rule-tests | Booking rules are tested at the command | stated-only | `create_booking.rs`'s `mod tests` tests the rules, and `routes.rs`'s tests check statuses, but nothing says why |
| docs | A module says what it is | taste | every module in the base opens with a `//!` line |

Two Python rules have no Rust case: `main-keywords` (Rust has no keyword arguments) and
`union-lines` (rustfmt puts one variant a line in every enum).
