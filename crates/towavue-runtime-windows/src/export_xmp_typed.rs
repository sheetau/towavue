use super::*;

pub(super) fn validate(field: MetadataField, text: &str) -> Result<(), ExportError> {
    let valid = match field {
        MetadataField::Date => date(text),
        MetadataField::Track => digits(text.strip_prefix(['+', '-']).unwrap_or(text).as_bytes()),
        _ => true,
    };
    if valid {
        Ok(())
    } else {
        Err(invalid(match field {
            MetadataField::Date => {
                "Date must be an XMP release date: YYYY, YYYY-MM, YYYY-MM-DD or YYYY-MM-DDThh:mm[:ss[.fraction]][Z or +/-hh:mm]"
            }
            _ => {
                "Track must be decimal digits with an optional leading + or - sign, not a track/total fraction"
            }
        }))
    }
}

fn digits(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.iter().all(u8::is_ascii_digit)
}

fn pair(bytes: &[u8]) -> Option<u8> {
    (bytes.len() == 2 && digits(bytes)).then(|| (bytes[0] - b'0') * 10 + bytes[1] - b'0')
}

fn date(text: &str) -> bool {
    let bytes = text.as_bytes();
    let Some(year) = bytes.get(..4).filter(|year| digits(year)) else {
        return false;
    };
    let year = year
        .iter()
        .fold(0u16, |value, digit| value * 10 + u16::from(digit - b'0'));
    if bytes.len() == 4 {
        return true;
    }
    let Some(month) = bytes
        .get(5..7)
        .and_then(pair)
        .filter(|month| (1..=12).contains(month))
    else {
        return false;
    };
    if bytes[4] != b'-' {
        return false;
    }
    if bytes.len() == 7 {
        return true;
    }
    let days = match month {
        2 if year.is_multiple_of(400) || (year.is_multiple_of(4) && !year.is_multiple_of(100)) => {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if bytes.get(7) != Some(&b'-')
        || !bytes
            .get(8..10)
            .and_then(pair)
            .is_some_and(|day| (1..=days).contains(&day))
    {
        return false;
    }
    if bytes.len() == 10 {
        return true;
    }
    if bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || !bytes
            .get(11..13)
            .and_then(pair)
            .is_some_and(|hour| hour <= 23)
        || !bytes
            .get(14..16)
            .and_then(pair)
            .is_some_and(|minute| minute <= 59)
    {
        return false;
    }
    let mut tail = &bytes[16..];
    if tail.first() == Some(&b':') {
        if !tail
            .get(1..3)
            .and_then(pair)
            .is_some_and(|second| second <= 59)
        {
            return false;
        }
        tail = &tail[3..];
        if tail.first() == Some(&b'.') {
            let count = tail[1..]
                .iter()
                .take_while(|byte| byte.is_ascii_digit())
                .count();
            if count == 0 {
                return false;
            }
            tail = &tail[count + 1..];
        }
    }
    tail.is_empty()
        || tail == b"Z"
        || (tail.len() == 6
            && matches!(tail[0], b'+' | b'-')
            && tail[3] == b':'
            && pair(&tail[1..3]).is_some_and(|hour| hour <= 23)
            && pair(&tail[4..6]).is_some_and(|minute| minute <= 59))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_accepts_precision_and_timezone_without_conversion() {
        for text in [
            "0000",
            "2026",
            "2026-09",
            "2000-02-29",
            "2024-02-29",
            "2026-09-12T08:30",
            "2026-09-12T08:30Z",
            "2026-09-12T08:30+09:00",
            "2026-09-12T08:30:59",
            "2026-09-12T08:30:59.0001-00:00",
            "2026-09-12T23:59:59.1+23:59",
        ] {
            validate(MetadataField::Date, text).unwrap_or_else(|error| panic!("{text}: {error}"));
        }
        for text in [
            "",
            "202",
            "20260",
            "2026-0",
            "2026-00",
            "2026-13",
            "2026-01-00",
            "2026-04-31",
            "1900-02-29",
            "2023-02-29",
            "2026-01-1",
            "2026-01-01Z",
            "2026-01T08:30",
            "2026-01-01 08:30",
            "2026-01-01T24:00",
            "2026-01-01T00:60",
            "2026-01-01T00:00:60",
            "2026-01-01T00:00.1Z",
            "2026-01-01T00:00:00.Z",
            "2026-01-01T00:00+24:00",
            "2026-01-01T00:00+00:60",
            "2026-01-01T00:00+0900",
            "2026-01-01T00:00z",
            "2026-01-01T00:00Zx",
            " 2026",
            "2026\n",
            "２０２６",
            "2026-日",
        ] {
            assert!(validate(MetadataField::Date, text).is_err(), "{text:?}");
        }
        for length in 0.."2026-09-12T08:30:59.1+09:00".len() {
            let _ = date(&"2026-09-12T08:30:59.1+09:00"[..length]);
        }
    }

    #[test]
    fn track_validates_lexically_without_machine_integer_limits() {
        for text in ["0", "+0002", "-2", &"9".repeat(1024)] {
            validate(MetadataField::Track, text).expect("XMP integer");
        }
        for text in [
            "", "+", "-", " 2", "2 ", "2\n", "2/12", "2.0", "1e2", "0x10", "１２", "++2", "--2",
        ] {
            assert!(validate(MetadataField::Track, text).is_err(), "{text:?}");
        }
        validate(MetadataField::Album, "2/12").expect("text fields unchanged");
        let mut options = MetadataExportOptions::default();
        options
            .set(MetadataField::Date, Some("circa 1999".into()))
            .expect("generic text");
        options
            .set(MetadataField::Track, Some("2/12".into()))
            .expect("generic text");
        ImageMetadataFormat::Png
            .validate_options(&options)
            .expect("PNG text remains unrestricted");
        assert!(
            ImageMetadataFormat::Jpeg
                .validate_options(&options)
                .is_err()
        );
    }
}
