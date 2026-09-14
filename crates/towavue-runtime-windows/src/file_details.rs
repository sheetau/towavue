use std::os::windows::fs::MetadataExt;
use std::path::Path;

use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::System::Time::{
    DYNAMIC_TIME_ZONE_INFORMATION, FileTimeToSystemTime, SystemTimeToTzSpecificLocalTimeEx,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileDetails {
    pub bytes: u64,
    /// File modification time in the system time zone, formatted to whole seconds.
    pub modified_local: Option<String>,
}

impl FileDetails {
    /// Perform one filesystem query. Call from a worker, not while drawing UI.
    pub fn read(path: &Path) -> std::io::Result<Self> {
        let metadata = path.metadata()?;
        Ok(Self {
            bytes: metadata.len(),
            modified_local: local_time(metadata.last_write_time(), None),
        })
    }
}

fn local_time(ticks: u64, zone: Option<&DYNAMIC_TIME_ZONE_INFORMATION>) -> Option<String> {
    let file_time = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut utc = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    // SAFETY: synchronous conversion with valid, distinct stack outputs; Windows
    // retains no pointers. A null zone selects the OS's dynamic daylight-saving rules.
    unsafe {
        FileTimeToSystemTime(&file_time, &mut utc).ok()?;
        SystemTimeToTzSpecificLocalTimeEx(zone.map(std::ptr::from_ref), &utc, &mut local).ok()?;
    }
    Some(format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        local.wYear, local.wMonth, local.wDay, local.wHour, local.wMinute, local.wSecond
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Time::{EnumDynamicTimeZoneInformation, SystemTimeToFileTime};

    fn zone(key: &str) -> DYNAMIC_TIME_ZONE_INFORMATION {
        for index in 0.. {
            let mut zone = DYNAMIC_TIME_ZONE_INFORMATION::default();
            // SAFETY: read-only enumeration into a valid stack output, not an OS setting change.
            assert_eq!(
                unsafe { EnumDynamicTimeZoneInformation(index, &mut zone) },
                0,
                "required Windows time zone: {key}"
            );
            let length = zone
                .TimeZoneKeyName
                .iter()
                .position(|unit| *unit == 0)
                .expect("zone name");
            if String::from_utf16(&zone.TimeZoneKeyName[..length]).expect("UTF-16 zone") == key {
                return zone;
            }
        }
        unreachable!()
    }

    fn ticks(year: u16, month: u16, day: u16, hour: u16) -> u64 {
        let time = SYSTEMTIME {
            wYear: year,
            wMonth: month,
            wDay: day,
            wHour: hour,
            wMinute: 34,
            wSecond: 56,
            ..Default::default()
        };
        let mut file_time = FILETIME::default();
        // SAFETY: valid stack input/output, retained only for the synchronous call.
        unsafe { SystemTimeToFileTime(&time, &mut file_time) }.expect("fixture time");
        u64::from(file_time.dwHighDateTime) << 32 | u64::from(file_time.dwLowDateTime)
    }

    #[test]
    fn file_dates_preserve_calendar_boundaries_and_daylight_saving_without_changing_os_settings() {
        let utc = zone("UTC");
        assert_eq!(
            local_time(ticks(2024, 2, 29, 12), Some(&utc)).as_deref(),
            Some("2024-02-29 12:34:56")
        );
        let japan = zone("Tokyo Standard Time");
        assert_eq!(
            local_time(ticks(2025, 12, 31, 20), Some(&japan)).as_deref(),
            Some("2026-01-01 05:34:56")
        );
        let pacific = zone("Pacific Standard Time");
        assert_eq!(
            local_time(ticks(2026, 1, 15, 12), Some(&pacific)).as_deref(),
            Some("2026-01-15 04:34:56")
        );
        assert_eq!(
            local_time(ticks(2026, 7, 15, 12), Some(&pacific)).as_deref(),
            Some("2026-07-15 05:34:56")
        );
        assert_eq!(
            local_time(ticks(2006, 3, 15, 12), Some(&pacific)).as_deref(),
            Some("2006-03-15 04:34:56")
        );
        assert_eq!(
            local_time(ticks(2026, 3, 15, 12), Some(&pacific)).as_deref(),
            Some("2026-03-15 05:34:56")
        );
        assert!(local_time(u64::MAX, Some(&utc)).is_none());
    }
}
