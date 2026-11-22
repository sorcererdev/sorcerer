//! A BookingRepository in a file (std::fs): one line a booking, fields separated by tabs.

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::PathBuf;

use crate::booking::models::booking::Booking;
use crate::booking::models::slot::{Day, Slot};
use crate::booking::models::table::Table;
use crate::booking::ports::booking_repository::BookingRepository;

pub struct FileBookingRepository {
    path: PathBuf,
    tables: Vec<Table>,
}

impl FileBookingRepository {
    pub fn new(path: impl Into<PathBuf>, tables: Vec<Table>) -> FileBookingRepository {
        FileBookingRepository {
            path: path.into(),
            tables,
        }
    }

    fn all(&self) -> Vec<Booking> {
        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == ErrorKind::NotFound => String::new(),
            Err(e) => panic!("cannot read {}: {e}", self.path.display()),
        };
        text.lines().filter_map(parse_line).collect()
    }
}

fn parse_line(line: &str) -> Option<Booking> {
    let fields: Vec<&str> = line.split('\t').collect();
    let [id, table_id, day, start, party, guest] = fields[..] else {
        return None;
    };
    Some(Booking {
        id: id.to_string(),
        table_id: table_id.to_string(),
        slot: Slot {
            day: day.parse().ok()?,
            start: start.parse().ok()?,
        },
        party: party.parse().ok()?,
        guest: guest.to_string(),
    })
}

impl BookingRepository for FileBookingRepository {
    fn table(&self, table_id: &str) -> Option<Table> {
        self.tables.iter().find(|t| t.id == table_id).cloned()
    }

    fn tables(&self) -> Vec<Table> {
        self.tables.clone()
    }

    fn booking_at(&self, table_id: &str, slot: &Slot) -> Option<Booking> {
        self.all()
            .into_iter()
            .find(|b| b.table_id == table_id && b.slot == *slot)
    }

    fn bookings_on(&self, day: Day) -> Vec<Booking> {
        let mut bookings: Vec<Booking> = self
            .all()
            .into_iter()
            .filter(|b| b.slot.day == day)
            .collect();
        bookings.sort_by(|a, b| (a.slot.start, &a.table_id).cmp(&(b.slot.start, &b.table_id)));
        bookings
    }

    fn save(&self, booking: Booking) {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .unwrap_or_else(|e| panic!("cannot open {}: {e}", self.path.display()));
        let line = format!(
            "{}\t{}\t{}\t{}\t{}\t{}\n",
            booking.id,
            booking.table_id,
            booking.slot.day,
            booking.slot.start,
            booking.party,
            booking.guest
        );
        file.write_all(line.as_bytes())
            .unwrap_or_else(|e| panic!("cannot write {}: {e}", self.path.display()));
    }
}
