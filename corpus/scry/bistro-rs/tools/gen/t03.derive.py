# t03 (T, adapters-main), hand-written on the base: the twin run (gen.py t03) made no change and said why (a library-side demo constructor would build an adapter outside main.rs); here main.rs gains --demo: the in-memory app with demo bookings made through the command
sub("main.rs", "//! Run one request: `bistro [--memory] <route> key=value…`, e.g.", "//! Run one request: `bistro [--memory | --demo] <route> key=value…` (`--demo`: in memory, with a\n//! few bookings tonight), e.g.")
sub("main.rs", "use bistro::booking::commands::create_booking::CreateBooking;\n", "use bistro::booking::commands::create_booking::{CreateBooking, CreateBookingInput};\nuse bistro::booking::models::slot::{Day, Slot, Time};\n")
sub("main.rs", """fn main() {""", """/// The in-memory app with a few bookings on DAY, made through the command.
fn demo(day: Day) -> App {
    let app = create(None);
    for (table_id, hour, party, guest) in [("t1", 18, 2, "Ada"), ("t2", 19, 4, "Grace"), ("t3", 20, 5, "Alan")] {
        let input = CreateBookingInput {
            table_id: table_id.into(),
            slot: Slot { day, start: Time::new(hour, 0) },
            party,
            guest: guest.into(),
        };
        if let Err(failure) = app.create_booking.handle(input) {
            eprintln!("demo booking not made: {}", failure.reason());
        }
    }
    app
}

fn main() {""")
sub("main.rs", """    let memory = args.first().is_some_and(|a| a == "--memory");
    if memory {
        args.remove(0);
    }""", """    let flag = args
        .first()
        .filter(|a| *a == "--memory" || *a == "--demo")
        .cloned();
    if flag.is_some() {
        args.remove(0);
    }""")
sub("main.rs", 'eprintln!("usage: bistro [--memory] <route> key=value…");', 'eprintln!("usage: bistro [--memory | --demo] <route> key=value…");')
sub("main.rs", """    let app = create(if memory {
        None
    } else {
        Some(Path::new("bistro.db"))
    });""", """    let app = match flag.as_deref() {
        Some("--demo") => demo(form.get("day").and_then(|d| d.parse().ok()).unwrap_or(Day::new(2026, 10, 9))),
        Some(_) => create(None),
        None => create(Some(Path::new("bistro.db"))),
    };""")
sub("main.rs", """    #[test]
    fn creating_the_app_writes_nothing_until_a_booking() {""", """    #[test]
    fn the_demo_app_has_bookings() {
        let page = dispatch(&demo(Day::new(2026, 10, 9)), "/bookings", &Form::from([("day", "2026-10-09")]));
        assert!(page.body.contains("Ada (2)"));
        assert!(page.body.contains("Alan (5)"));
    }

    #[test]
    fn creating_the_app_writes_nothing_until_a_booking() {""")
