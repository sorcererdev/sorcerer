# bistro-rs: scry's Rust corpus

The second corpus of scry's eval: the same restaurant-bookings project as the Python corpus
(`corpus/scry/base/`), written in Rust, with its own frozen conventions and cases.

## The base

`base/` is a std-only Rust crate (package `bistro`, edition 2024, no dependencies). It is its own
workspace root (`[workspace]` in its `Cargo.toml`), so the sorcerer workspace never builds it, and every
check runs offline in seconds.

```
src/lib.rs                          booking + web
src/main.rs                         the composition root: adapters, commands and queries, the app;
                                    dispatches one request from the command line
src/booking/models/                 Booking, Slot (Day, Time, Weekday), Table: facts, no policy
src/booking/ports/booking_repository/  the BookingRepository trait (mod.rs, with the adapters' shared
                                    contract test), in_memory.rs, file.rs (std::fs, one line a booking)
src/booking/ports/id_generator/     the IdGenerator trait, in_memory.rs (SequentialIds: b1, b2, …),
                                    time_ids.rs (TimeIds: ids for real use)
src/booking/commands/create_booking.rs  CreateBooking::handle -> Result<Booking, CreateBookingFailure>,
                                    the booking rules, and their tests against the in-memory adapter
src/booking/queries/get_day_schedule.rs GetDaySchedule::handle -> DaySchedule
src/web/mod.rs                      Form, Response
src/web/app.rs                      App (the commands and queries it is handed), home, dispatch
src/web/bookings/{routes,failures,pages}.rs  routes as functions over a Form; each failure's status;
                                    pages as plain text; route tests in routes.rs
```

It ports the Python base module by module, keeping names and shapes (`CreateBooking`, the three
failures `SlotTaken`, `PartyTooLarge`, `TableNotFound`, `GetDaySchedule`); where the languages differ:
the SQLite adapter is a file adapter over `std::fs`; the web layer has no HTTP server (routes are
functions from a `Form` to a `Response { status, body }`, and `main.rs` dispatches one request:
`bistro [--memory] /bookings/create_booking table_id=t1 day=2026-10-09 start=19:00 party=2 guest=Ada`);
failures are an enum returned in `Err`; commands and queries hold their ports as `Rc<dyn Port>`, and
the ports' methods take `&self` (the in-memory adapters use `RefCell`/`Cell`); tests are inline
`#[cfg(test)] mod tests`, not `__test` siblings. As in Python, the day page defaults to today, so the
web reads the clock.

Check it (from the repository root; the target directory keeps `target/` out of the corpus):

```
export CARGO_TARGET_DIR=$TMPDIR/bistro-rs-target
cargo check  --manifest-path corpus/scry/bistro-rs/base/Cargo.toml --offline --locked --all-targets
cargo test   --manifest-path corpus/scry/bistro-rs/base/Cargo.toml --offline --locked
cargo clippy --manifest-path corpus/scry/bistro-rs/base/Cargo.toml --offline --locked --all-targets -- -D warnings
cargo fmt    --manifest-path corpus/scry/bistro-rs/base/Cargo.toml --check
```

### Units on the base

What `sorcerer-core`'s `units_of` makes of the base's shapes (checked on `create_booking.rs`,
`in_memory.rs`, `routes.rs` and `id_generator/in_memory.rs`), which is where a finding can be placed:

- a method in `impl Type` is `Type::method` (`CreateBooking::handle`), and so is a method in
  `impl Trait for Type` (`InMemoryBookingRepository::save`): the trait is not in the name; the `impl`
  block is a unit of its own, named by the type, starting at the `impl …` line;
- a function in `mod tests` is `tests::name` (`tests::books_a_free_slot`), inside a `tests` unit
  (kind `module`) starting at the `mod tests {` line;
- a free function is its name (`create_booking`, `today`);
- a struct or enum is a unit named by itself.

One difference from Python: a unit starts at its `fn` line, so a `#[test]` (or any attribute) line
above it is outside the unit, where Python's decorators are inside theirs.
