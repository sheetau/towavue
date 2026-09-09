use towavue_core::{EditState, MediaTime};

pub fn label(state: &EditState, duration: MediaTime) -> Option<String> {
    if state.trim_start.is_none() && state.trim_end.is_none() {
        return None;
    }
    Some(format!(
        "Trim {} – {} · playback and export",
        timestamp(state.trim_start.unwrap_or(MediaTime::ZERO)),
        timestamp(state.trim_end.unwrap_or(duration)),
    ))
}

fn timestamp(time: MediaTime) -> String {
    let millis = time.as_nanoseconds().max(0) / 1_000_000;
    format!(
        "{:02}:{:02}.{:03}",
        millis / 60_000,
        millis / 1_000 % 60,
        millis % 1_000
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_label_preserves_subsecond_endpoints_and_implicit_source_edges() {
        let duration = MediaTime::from_nanoseconds(30_000_000_000);
        let mut state = EditState::default();
        assert_eq!(label(&state, duration), None);
        state.trim_end = Some(MediaTime::from_nanoseconds(2_833_333_333));
        assert_eq!(
            label(&state, duration).as_deref(),
            Some("Trim 00:00.000 – 00:02.833 · playback and export")
        );
        state.trim_start = state.trim_end.take();
        assert_eq!(
            label(&state, duration).as_deref(),
            Some("Trim 00:02.833 – 00:30.000 · playback and export")
        );
    }
}
