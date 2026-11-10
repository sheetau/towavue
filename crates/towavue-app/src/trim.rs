use towavue_core::localization::{Language, formatted};
use towavue_core::{EditState, MediaTime};

pub fn label(language: Language, state: &EditState, duration: MediaTime) -> Option<String> {
    if state.trim_start.is_none() && state.trim_end.is_none() {
        return None;
    }
    Some(formatted::trim_edited(
        language,
        &crate::format_time_precise(state.trim_start.unwrap_or(MediaTime::ZERO)),
        &crate::format_time_precise(state.trim_end.unwrap_or(duration)),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_label_preserves_subsecond_endpoints_and_implicit_source_edges() {
        let duration = MediaTime::from_nanoseconds(30_000_000_000);
        let mut state = EditState::default();
        assert_eq!(label(Language::English, &state, duration), None);
        state.trim_end = Some(MediaTime::from_nanoseconds(2_833_333_333));
        assert_eq!(
            label(Language::English, &state, duration).as_deref(),
            Some("Trim 0:00:000 – 0:02:833 · playback and export")
        );
        state.trim_start = state.trim_end.take();
        assert_eq!(
            label(Language::English, &state, duration).as_deref(),
            Some("Trim 0:02:833 – 0:30:000 · playback and export")
        );
        state.trim_start = Some(MediaTime::from_nanoseconds(3_600_123_999_999));
        assert_eq!(
            label(
                Language::English,
                &state,
                MediaTime::from_nanoseconds(7_205_987_000_000)
            )
            .as_deref(),
            Some("Trim 1:00:00:123 – 2:00:05:987 · playback and export")
        );
        assert_eq!(
            label(
                Language::Japanese,
                &state,
                MediaTime::from_nanoseconds(7_205_987_000_000)
            )
            .as_deref(),
            Some("トリミング 1:00:00:123 – 2:00:05:987 · 再生と書き出しに適用")
        );
    }
}
