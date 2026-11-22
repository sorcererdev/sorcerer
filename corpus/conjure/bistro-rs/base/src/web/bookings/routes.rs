//! Bookings routes: the day page, and booking a table.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::booking::commands::create_booking::CreateBookingInput;
use crate::booking::models::slot::{Day, Slot, Time};
use crate::web::app::App;
use crate::web::bookings::failures::create_booking_status;
use crate::web::bookings::pages;
use crate::web::{Form, Response};

fn today() -> Day {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    Day::from_days_since_epoch((seconds / 86_400) as i64)
}

fn field<'a>(form: &'a Form, key: &str) -> Result<&'a str, Response> {
    form.get(key)
        .ok_or_else(|| Response::new(400, format!("missing {key}")))
}

fn parsed<T: std::str::FromStr<Err = String>>(form: &Form, key: &str) -> Result<T, Response> {
    field(form, key)?
        .parse()
        .map_err(|e: String| Response::new(400, e))
}

pub fn day_page(form: &Form, app: &App) -> Response {
    let day = match form.get("day") {
        Some(_) => match parsed::<Day>(form, "day") {
            Ok(day) => day,
            Err(response) => return response,
        },
        None => today(),
    };
    let schedule = app.get_day_schedule.handle(day);
    Response::new(200, pages::day_page(&schedule))
}

fn create_booking_input(form: &Form) -> Result<CreateBookingInput, Response> {
    let party = field(form, "party")?
        .parse()
        .map_err(|_| Response::new(400, "party is not a number"))?;
    Ok(CreateBookingInput {
        table_id: field(form, "table_id")?.to_string(),
        slot: Slot {
            day: parsed::<Day>(form, "day")?,
            start: parsed::<Time>(form, "start")?,
        },
        party,
        guest: field(form, "guest")?.to_string(),
    })
}

pub fn create_booking(form: &Form, app: &App) -> Response {
    let input = match create_booking_input(form) {
        Ok(input) => input,
        Err(response) => return response,
    };
    match app.create_booking.handle(input) {
        Ok(booking) => Response::new(201, pages::booking_page(&booking)),
        Err(failure) => Response::new(
            create_booking_status(&failure),
            pages::failure_page(failure.reason()),
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;
    use crate::booking::commands::create_booking::CreateBooking;
    use crate::booking::models::table::Table;
    use crate::booking::ports::booking_repository::in_memory::InMemoryBookingRepository;
    use crate::booking::ports::id_generator::in_memory::SequentialIds;
    use crate::booking::queries::get_day_schedule::GetDaySchedule;

    fn app() -> App {
        let bookings = Rc::new(InMemoryBookingRepository::new(vec![Table {
            id: "t1".into(),
            seats: 4,
        }]));
        App {
            create_booking: CreateBooking::new(bookings.clone(), Rc::new(SequentialIds::default())),
            get_day_schedule: GetDaySchedule::new(bookings),
        }
    }

    fn form(changes: &[(&str, &str)]) -> Form {
        let mut form = Form::from([
            ("table_id", "t1"),
            ("day", "2026-10-09"),
            ("start", "19:00"),
            ("party", "2"),
            ("guest", "Ada"),
        ]);
        for (k, v) in changes {
            form.0.insert(k.to_string(), v.to_string());
        }
        form
    }

    #[test]
    fn the_day_page_lists_the_tables() {
        let page = day_page(&Form::from([("day", "2026-10-09")]), &app());
        assert_eq!(page.status, 200);
        assert!(page.body.contains("t1"));
    }

    #[test]
    fn a_booking_is_created() {
        let response = create_booking(&form(&[]), &app());
        assert_eq!(response.status, 201);
        assert!(response.body.contains("Ada"));
    }

    #[test]
    fn a_taken_slot_is_a_conflict() {
        let app = app();
        create_booking(&form(&[]), &app);
        assert_eq!(
            create_booking(&form(&[("guest", "Grace")]), &app).status,
            409
        );
    }

    #[test]
    fn a_party_too_large_is_unprocessable() {
        assert_eq!(create_booking(&form(&[("party", "9")]), &app()).status, 422);
    }

    #[test]
    fn an_unknown_table_is_not_found() {
        assert_eq!(
            create_booking(&form(&[("table_id", "t9")]), &app()).status,
            404
        );
    }

    #[test]
    fn a_malformed_day_is_a_bad_request() {
        assert_eq!(
            create_booking(&form(&[("day", "tomorrow")]), &app()).status,
            400
        );
    }
}
