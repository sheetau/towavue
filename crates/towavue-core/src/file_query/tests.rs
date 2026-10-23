use super::*;

fn query(text: &str) -> FileQuery {
    FileQuery::new(text, Some((2026, 10, 23)))
}
fn matches(text: &str, path: &str) -> bool {
    query(text).matches_path(Path::new(path), false)
}

#[test]
fn boolean_precedence_grouping_and_quoted_phrases_follow_everything() {
    assert!(matches("abc 123|456", "abc456.mp4"));
    assert!(!matches("abc 123|456", "456.mp4"));
    assert!(matches("<abc 123>|456", "456.mp4"));
    assert!(!matches("!<abc 123>", "abc123.mp4"));
    assert!(matches("!<abc 123>", "abc456.mp4"));
    assert!(matches("!!abc", "abc.mp4"));
    assert!(matches("\"abc 123\"", "pre-abc 123-post.mp4"));
    assert!(!matches("\"abc 123\"", "abc---123.mp4"));
    assert!(matches("\"!draft\"", "!draft.mp4"));
    assert!(matches("whole:\"abc 123.mp4\"", "abc 123.mp4"));
    assert!(!matches("whole:abc", "abc.mp4"));
    assert!(!matches("abc 123", "a---b---c123.mp4"));
    assert_eq!(query("abc").score(Path::new("a---b---c.mp4")), Some(3));
    assert_eq!(query("\"abc\"").score(Path::new("a---b---c.mp4")), None);
}

#[test]
fn extensions_types_paths_and_folder_boundaries_compose() {
    for text in [
        "ext:mp4;mkv",
        "ext:<mp4 | mkv>",
        "video: !audio:",
        "*.MP4",
        "name:clip*",
        "stem:clip",
    ] {
        assert!(matches(text, r"C:\Media\clip.MP4"), "{text}");
    }
    assert!(matches("audio:|image:", "song.flac"));
    assert!(!matches("image:", "song.flac"));
    assert!(!matches("ext:mp", "clip.mp4"));
    assert!(matches(
        r#"folder:"C:\My Media" *.mp4"#,
        r"C:\My Media\child\clip.mp4"
    ));
    assert!(!matches(
        r#"folder:"C:\My Media""#,
        r"C:\My Media2\clip.mp4"
    ));
    assert!(matches(r"parent:C:\Media", r"C:\Media\clip.mp4"));
    assert!(!matches(r"parent:C:\Media", r"C:\Media\child\clip.mp4"));
    assert!(matches(r"c:\media\", r"C:\Media\clip.mp4"));
    assert!(!matches("media", r"C:\Media\clip.mp4"));
    assert!(query("media").matches_gallery(Path::new(r"C:\Media\clip.mp4"), None));
    assert!(query("*.mp4").matches_gallery(Path::new(r"C:\Media\clip.mp4"), None));
}

#[test]
fn wildcards_match_whole_names_and_unicode_without_crossing_separators() {
    assert!(matches("?.png", "猫.png"));
    assert!(!matches("?.png", "子猫.png"));
    assert!(!matches("cat*", "a-cat.png"));
    assert!(matches("*cat*", "a-cat.png"));
    assert!(matches(r"path:c:\**\?.png", r"C:\one\two\猫.png"));
    assert!(!matches(r"path:c:\*\?.png", r"C:\one\two\猫.png"));
    assert!(matches("\"a b*.png\"", "a b cat.png"));
}

#[test]
fn gallery_dates_use_inclusive_calendar_ranges_and_unknown_is_explicit() {
    let path = Path::new("photo.png");
    for text in [
        "dg:today",
        "dg:2026/10/23",
        "dg:2026-10",
        "dg:2026",
        "dg:2026/01/01..2026/10/23",
        "dg:>=2026-10-23",
        "dg:<2026-10-24",
        "date-gallery:thisweek",
    ] {
        assert!(
            query(text).matches_gallery(path, Some((2026, 10, 23))),
            "{text}"
        );
        assert!(!query(text).matches_path(path, false));
    }
    assert!(query("dg:lastweek").matches_gallery(path, Some((2026, 10, 12))));
    assert!(query("dg:lastweek").matches_gallery(path, Some((2026, 10, 18))));
    assert!(!query("dg:lastweek").matches_gallery(path, Some((2026, 10, 19))));
    assert!(!query("dg:>2026-10").matches_gallery(path, Some((2026, 10, 31))));
    assert!(query("dg:>2026-10").matches_gallery(path, Some((2026, 11, 1))));
    assert!(query("dg:2024-02-29").matches_gallery(path, Some((2024, 2, 29))));
    assert!(query("dg:unknown").matches_gallery(path, None));
    assert!(!query("dg:unknown").matches_path(path, false));
    assert!(!query("dg:today").matches_gallery(path, None));
    assert!(query("dg:!=today").matches_gallery(path, Some((2026, 10, 22))));
}

#[test]
fn incomplete_invalid_and_excessive_queries_fail_without_panicking_or_broadening() {
    for text in [
        "abc |",
        "!",
        "<abc",
        "abc>",
        "\"abc",
        "ext:",
        "dg:2025-02-29",
        "dg:2026-13",
        "dg:2026..2025",
        "size:>10mb",
        "dm:today",
        "dc:today",
    ] {
        assert!(query(text).error().is_some(), "{text}");
        assert!(!matches(text, "abc.png"));
    }
    assert_eq!(
        query(&"!".repeat(1000)).error(),
        Some(FileQueryError::TooComplex)
    );
    assert_eq!(
        query(&"x".repeat(8193)).error(),
        Some(FileQueryError::TooComplex)
    );
    assert!(matches("", "anything.png"));
    assert!(matches("未知", "未知の写真.png"));
}
