//! The composition root: adapters, then commands and queries, then the web app.
//!
//! Run one request: `bistro [--memory] <route> key=value…`, e.g.
//! `bistro /bookings/create_booking table_id=t1 day=2026-10-09 start=19:00 party=2 guest=Ada`.

use std::path::Path;
use std::rc::Rc;

use bistro::booking::commands::create_booking::CreateBooking;
use bistro::booking::models::table::Table;
use bistro::booking::ports::booking_repository::BookingRepository;
use bistro::booking::ports::booking_repository::file::FileBookingRepository;
use bistro::booking::ports::booking_repository::in_memory::InMemoryBookingRepository;
use bistro::booking::ports::id_generator::time_ids::TimeIds;
use bistro::booking::queries::get_day_schedule::GetDaySchedule;
use bistro::web::Form;
use bistro::web::app::{App, dispatch};

fn tables() -> Vec<Table> {
    vec![
        Table {
            id: "t1".into(),
            seats: 2,
        },
        Table {
            id: "t2".into(),
            seats: 4,
        },
        Table {
            id: "t3".into(),
            seats: 6,
        },
    ]
}

fn create(db_path: Option<&Path>) -> App {
    // adapters
    let booking_repository: Rc<dyn BookingRepository> = match db_path {
        Some(path) => Rc::new(FileBookingRepository::new(path, tables())),
        None => Rc::new(InMemoryBookingRepository::new(tables())),
    };
    let id_generator = Rc::new(TimeIds::default());
    // commands and queries
    App {
        create_booking: CreateBooking::new(booking_repository.clone(), id_generator),
        get_day_schedule: GetDaySchedule::new(booking_repository),
    }
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let memory = args.first().is_some_and(|a| a == "--memory");
    if memory {
        args.remove(0);
    }
    let Some(route) = args.first().cloned() else {
        eprintln!("usage: bistro [--memory] <route> key=value…");
        std::process::exit(2);
    };
    let form = Form(
        args[1..]
            .iter()
            .filter_map(|a| a.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
    );
    let app = create(if memory {
        None
    } else {
        Some(Path::new("bistro.db"))
    });
    let response = dispatch(&app, &route, &form);
    println!("{}\n{}", response.status, response.body);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creating_the_app_writes_nothing_until_a_booking() {
        let db = std::env::temp_dir().join(format!("bistro-main-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&db);
        let app = create(Some(&db));
        assert!(!db.exists());
        let form = Form::from([
            ("table_id", "t2"),
            ("day", "2026-10-09"),
            ("start", "19:00"),
            ("party", "2"),
            ("guest", "Ada"),
        ]);
        assert_eq!(
            dispatch(&app, "/bookings/create_booking", &form).status,
            201
        );
        assert!(db.exists());
        let _ = std::fs::remove_file(&db);
    }
}
