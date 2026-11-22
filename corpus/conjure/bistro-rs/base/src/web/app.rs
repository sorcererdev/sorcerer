//! The web app: the commands and queries it was handed, the home page, and dispatch to each area's routes.

use crate::booking::commands::create_booking::CreateBooking;
use crate::booking::queries::get_day_schedule::GetDaySchedule;
use crate::web::bookings::routes;
use crate::web::{Form, Response};

/// The app's areas, one per web/*/routes.rs.
pub const RESOURCES: &[&str] = &["bookings"];

pub struct App {
    pub create_booking: CreateBooking,
    pub get_day_schedule: GetDaySchedule,
}

pub fn home() -> Response {
    let links: Vec<String> = RESOURCES.iter().map(|r| format!("- /{r}")).collect();
    Response::new(200, format!("bistro\n{}", links.join("\n")))
}

pub fn dispatch(app: &App, route: &str, form: &Form) -> Response {
    match route {
        "/" => home(),
        "/bookings" => routes::day_page(form, app),
        "/bookings/create_booking" => routes::create_booking(form, app),
        _ => Response::new(404, format!("no route {route}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_home_page_names_the_app() {
        let page = home();
        assert_eq!(page.status, 200);
        assert!(page.body.contains("bistro"));
    }
}
