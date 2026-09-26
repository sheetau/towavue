use super::*;
use towavue_core::localization::Language;

#[test]
fn actual_format_validation_errors_translate_without_mutating_inputs() {
    let root = audio_tests::root("localized-validation");
    let cancel = AtomicBool::new(false);
    let bad = root.join("日本語{invalid}.png");
    let jpeg = bad.with_extension("jpg");
    let webp = bad.with_extension("webp");
    let empty = root.join("empty.avif");
    for path in [&bad, &jpeg, &webp] {
        fs::write(path, [0_u8; 16]).expect("owned malformed input");
    }
    fs::write(&empty, []).expect("owned empty input");
    let failures = [
        (
            png_metadata::inspect(&bad).expect_err("invalid PNG"),
            "PNG metadata: input is not PNG",
            "入力がPNGではありません",
        ),
        (
            jpeg_metadata::inspect(&jpeg).expect_err("invalid JPEG"),
            "JPEG metadata: input is not JPEG",
            "入力がJPEGではありません",
        ),
        (
            webp_metadata::inspect(&webp).expect_err("invalid WebP"),
            "WebP metadata: invalid RIFF/WEBP header or length",
            "RIFF/WEBPのヘッダーまたは長さが不正です",
        ),
        (
            avif::Animation::read(&empty, &cancel).expect_err("invalid AVIF"),
            "AVIF export: missing ftyp",
            "ftypがありません",
        ),
        (
            xmp::parse(b"<bad/>", &cancel).expect_err("invalid XMP root"),
            "XMP metadata: expected XMP RDF root",
            "XMPのRDFルート要素が必要です",
        ),
        (
            gif_animation::Animation::from_milliseconds(1, &[1])
                .expect_err("unrepresentable delay"),
            "GIF export: 1 ms frame delay cannot be represented exactly in GIF; use WebP output",
            "1 msのフレーム遅延をGIFで正確に表現できません。WebP出力",
        ),
    ];
    for (error, english, japanese_detail) in failures {
        assert_eq!(
            error.to_string(),
            format!("FFmpeg export failed: {english}")
        );
        assert_eq!(error.message(Language::English), error.to_string());
        assert!(error.message(Language::Japanese).contains(japanese_detail));
    }
    for path in [&bad, &jpeg, &webp] {
        assert_eq!(fs::read(path).expect("unchanged input"), [0; 16]);
    }
    assert!(fs::read(&empty).expect("unchanged empty input").is_empty());
    fs::remove_dir_all(root).expect("owned fixture cleanup");
}
