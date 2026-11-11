use super::*;
use towavue_core::{TimeRange, TimelineEdit};

fn range(start: u64, end: u64) -> TimeRange {
    TimeRange::new(
        media_time(Duration::from_millis(start)),
        media_time(Duration::from_millis(end)),
    )
    .expect("range")
}

#[test]
fn source_peak_pyramid_matches_every_scalar_interval_and_retains_transients() {
    let mut detail = Detail::default();
    let values: Vec<_> = (0..1025)
        .map(|i| {
            if i == 777 {
                4.0
            } else {
                (i % 17) as f32 / 20.0
            }
        })
        .collect();
    detail.install(values.clone());
    for start in (0..values.len()).step_by(7) {
        for length in [1, 2, 3, 17, 129, 1025] {
            let end = (start + length).min(values.len());
            let expected = values[start..end].iter().copied().fold(0.0, f32::max);
            assert_eq!(
                detail.peak(
                    start as f64 / values.len() as f64,
                    end as f64 / values.len() as f64
                ),
                expected
            );
        }
    }
}

#[test]
fn committed_gain_stretch_delete_and_undo_reuse_original_amplitudes() {
    let source =
        EditTimeline::new(media_time(Duration::from_secs(4)), Default::default()).expect("source");
    let key = Arc::new(Key {
        path: "owned-envelope.wav".into(),
        track: None,
        plan: source.clone(),
        columns: 4,
    });
    let mut detail = Detail {
        key: Some(key.clone()),
        display_volume: 1.0,
        ..Default::default()
    };
    detail.install(vec![0.2, 0.6, 1.4, 0.9]);
    let retained = detail.values.clone().expect("source");
    let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 100.0));
    let before = detail.mesh(rect, 1.0, None).expect("before");
    let preview = detail
        .mesh(rect, 1.0, Some((range(1000, 3000), 0.5)))
        .expect("preview");
    let mut plan = source.clone();
    assert!(plan.apply(TimelineEdit::ScaleVolume(range(1000, 3000), 0.5)));
    detail.display_plan = Some(plan.clone());
    let committed = detail.mesh(rect, 1.0, None).expect("commit");
    assert_eq!(
        preview.vertices, committed.vertices,
        "release retains the detailed waveform immediately"
    );
    assert!(plan.apply(TimelineEdit::Stretch(
        range(1000, 3000),
        media_time(Duration::from_secs(4))
    )));
    detail.display_plan = Some(plan.clone());
    let stretched = detail.mesh(rect, 1.0, None).expect("stretch");
    assert_ne!(stretched.vertices, committed.vertices);
    assert!(plan.apply(TimelineEdit::Delete(range(0, 1000))));
    detail.display_plan = Some(plan);
    assert!(
        !detail
            .mesh(rect, 1.0, None)
            .expect("delete")
            .vertices
            .is_empty()
    );
    detail.display_plan = Some(source);
    assert_eq!(
        before.vertices,
        detail.mesh(rect, 1.0, None).expect("undo").vertices
    );
    assert!(Arc::ptr_eq(
        &retained,
        detail.values.as_ref().expect("same data")
    ));
    assert!(Arc::ptr_eq(&key, detail.key.as_ref().expect("same source")));
}
