//! UTC date and time for the record, from the system clock, with no date crate.

use std::time::{SystemTime, UNIX_EPOCH};

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// A moment in UTC, to the second.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Now {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl Now {
    pub fn system() -> Self {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        Self::from_unix(secs)
    }

    pub fn from_unix(secs: u64) -> Self {
        let days = (secs / 86_400) as i64;
        let rem = (secs % 86_400) as u32;
        let (year, month, day) = civil_from_days(days);
        Self {
            year,
            month,
            day,
            hour: rem / 3600,
            minute: rem / 60 % 60,
            second: rem % 60,
        }
    }

    /// `2026-10-28`, for file names.
    pub fn iso_date(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// `2026-10-28T09:05:00Z`.
    pub fn iso_timestamp(&self) -> String {
        format!(
            "{}T{:02}:{:02}:{:02}Z",
            self.iso_date(),
            self.hour,
            self.minute,
            self.second
        )
    }

    /// `090500`, to keep a second record on the same day apart from the first.
    pub fn compact_time(&self) -> String {
        format!("{:02}{:02}{:02}", self.hour, self.minute, self.second)
    }

    /// `28 October 2026`, for prose.
    pub fn long_date(&self) -> String {
        format!(
            "{} {} {}",
            self.day,
            MONTHS[(self.month - 1) as usize],
            self.year
        )
    }
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's
/// `civil_from_days`).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_known_moments() {
        assert_eq!(Now::from_unix(0).iso_timestamp(), "1970-01-01T00:00:00Z");
        // `date -u -d @1793178305`
        let n = Now::from_unix(1_793_178_305);
        assert_eq!(n.iso_timestamp(), "2026-10-28T09:05:05Z");
        assert_eq!(n.long_date(), "28 October 2026");
        assert_eq!(n.compact_time(), "090505");
        // A leap day.
        assert_eq!(Now::from_unix(1_709_164_800).iso_date(), "2024-02-29");
        assert_eq!(Now::from_unix(951_782_400).iso_date(), "2000-02-29");
    }
}
