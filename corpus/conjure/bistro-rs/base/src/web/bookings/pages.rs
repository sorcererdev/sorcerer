//! The bookings pages, rendered as plain text.

use std::collections::BTreeMap;

use crate::booking::models::booking::Booking;
use crate::booking::models::slot::Time;
use crate::booking::queries::get_day_schedule::DaySchedule;

/// The day page's columns: every half hour from 17:00 to 21:30.
pub fn slots() -> Vec<Time> {
    (17..22)
        .flat_map(|h| [Time::new(h, 0), Time::new(h, 30)])
        .collect()
}

pub fn day_page(schedule: &DaySchedule) -> String {
    let booked: BTreeMap<(&str, Time), &Booking> = schedule
        .bookings
        .iter()
        .map(|b| ((b.table_id.as_str(), b.slot.start), b))
        .collect();
    let mut page = format!("Bookings on {}\n", schedule.day);
    for table in &schedule.tables {
        let cells: Vec<String> = slots()
            .into_iter()
            .map(|start| match booked.get(&(table.id.as_str(), start)) {
                Some(b) => format!("{start} {} ({})", b.guest, b.party),
                None => format!("{start} free"),
            })
            .collect();
        page.push_str(&format!(
            "{} (seats {}): {}\n",
            table.id,
            table.seats,
            cells.join(" | ")
        ));
    }
    page
}

pub fn booking_page(booking: &Booking) -> String {
    format!(
        "Booked: {} for {} at table {} on {} at {}",
        booking.guest, booking.party, booking.table_id, booking.slot.day, booking.slot.start
    )
}

pub fn failure_page(reason: &str) -> String {
    format!("Not booked: {reason}")
}
