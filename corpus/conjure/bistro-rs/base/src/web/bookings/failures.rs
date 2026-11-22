//! Each command's failures as HTTP statuses: an exhaustive match, so a new failure needs a status here.

use crate::booking::commands::create_booking::CreateBookingFailure;

pub fn create_booking_status(failure: &CreateBookingFailure) -> u16 {
    match failure {
        CreateBookingFailure::SlotTaken { .. } => 409,
        CreateBookingFailure::PartyTooLarge { .. } => 422,
        CreateBookingFailure::TableNotFound { .. } => 404,
    }
}
