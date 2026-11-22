//! A half-hour slot on a day, and the day and time it is made of.

use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Day {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl Day {
    pub fn new(year: i32, month: u32, day: u32) -> Day {
        Day { year, month, day }
    }

    /// The day `days` after 1970-01-01 (negative before it).
    pub fn from_days_since_epoch(days: i64) -> Day {
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
        Day { year, month, day }
    }

    pub fn days_since_epoch(&self) -> i64 {
        let year = i64::from(self.year) - i64::from(self.month <= 2);
        let era = year.div_euclid(400);
        let yoe = year.rem_euclid(400);
        let month = i64::from(self.month);
        let mp = if month > 2 { month - 3 } else { month + 9 };
        let doy = (153 * mp + 2) / 5 + i64::from(self.day) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    pub fn weekday(&self) -> Weekday {
        const DAYS: [Weekday; 7] = [
            Weekday::Thursday,
            Weekday::Friday,
            Weekday::Saturday,
            Weekday::Sunday,
            Weekday::Monday,
            Weekday::Tuesday,
            Weekday::Wednesday,
        ];
        DAYS[self.days_since_epoch().rem_euclid(7) as usize]
    }
}

impl fmt::Display for Day {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl FromStr for Day {
    type Err = String;

    fn from_str(s: &str) -> Result<Day, String> {
        let bad = || format!("not a day: {s}");
        let mut parts = s.splitn(3, '-');
        let year = parts.next().and_then(|p| p.parse().ok()).ok_or_else(bad)?;
        let month: u32 = parts.next().and_then(|p| p.parse().ok()).ok_or_else(bad)?;
        let day: u32 = parts.next().and_then(|p| p.parse().ok()).ok_or_else(bad)?;
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return Err(bad());
        }
        Ok(Day { year, month, day })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Time {
    pub hour: u32,
    pub minute: u32,
}

impl Time {
    pub fn new(hour: u32, minute: u32) -> Time {
        Time { hour, minute }
    }
}

impl fmt::Display for Time {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}", self.hour, self.minute)
    }
}

impl FromStr for Time {
    type Err = String;

    fn from_str(s: &str) -> Result<Time, String> {
        let bad = || format!("not a time: {s}");
        let (hour, minute) = s.split_once(':').ok_or_else(bad)?;
        let hour: u32 = hour.parse().map_err(|_| bad())?;
        let minute: u32 = minute.parse().map_err(|_| bad())?;
        if hour > 23 || minute > 59 {
            return Err(bad());
        }
        Ok(Time { hour, minute })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Slot {
    pub day: Day,
    pub start: Time,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_day_knows_its_weekday() {
        assert_eq!(Day::new(2026, 10, 9).weekday(), Weekday::Friday);
        assert_eq!(Day::new(1970, 1, 1).weekday(), Weekday::Thursday);
    }

    #[test]
    fn days_since_epoch_round_trips() {
        for days in [-1, 0, 20_000, 20_735] {
            assert_eq!(Day::from_days_since_epoch(days).days_since_epoch(), days);
        }
        assert_eq!(Day::from_days_since_epoch(0), Day::new(1970, 1, 1));
    }

    #[test]
    fn days_and_times_parse_and_print() {
        let day: Day = "2026-10-09".parse().unwrap();
        assert_eq!(day.to_string(), "2026-10-09");
        let time: Time = "19:30".parse().unwrap();
        assert_eq!(time.to_string(), "19:30");
        assert!("2026-13-01".parse::<Day>().is_err());
        assert!("25:00".parse::<Time>().is_err());
    }
}
