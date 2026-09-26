//! The helper's error record is diagnostic data, never installation authority.
use std::borrow::Cow;
use towavue_core::localization::{Language, Text, formatted};

const HEADER: &str = "towavue-update-error-v1\n";

// Stable IDs, not English text, select translations. Unknown/old records and
// native diagnostics stay literal. This does not change the helper state file.
const REASONS: &[(&str, Text)] = &[
    ("path-type", Text::HelperPathType),
    ("local-path", Text::HelperLocalPath),
    ("metadata-limit", Text::HelperMetadataLimit),
    ("state-changed", Text::HelperStateChanged),
    ("hex-metadata", Text::HelperHexMetadata),
    ("stable-version", Text::HelperStableVersion),
    ("signature-size", Text::HelperSignatureSize),
    ("signature", Text::HelperSignature),
    ("manifest", Text::HelperManifest),
    ("payload-size", Text::HelperPayloadSize),
    ("payload-hash", Text::HelperPayloadHash),
    ("installation", Text::HelperInstallation),
    ("generation", Text::HelperGeneration),
    ("parent", Text::HelperParent),
    ("not-newer", Text::HelperNotNewer),
    ("shutdown-timeout", Text::HelperShutdownTimeout),
    ("installed-version", Text::HelperInstalledVersion),
];

pub(super) fn message(language: Language, record: &str) -> Cow<'_, str> {
    let Some(payload) = record.strip_prefix(HEADER) else {
        return Cow::Borrowed(record);
    };
    let Some((reason, rest)) = payload.split_once('\n') else {
        return Cow::Borrowed(record);
    };
    let Some((argument, diagnostic)) = rest.split_once('\n') else {
        return Cow::Borrowed(record);
    };
    if diagnostic.is_empty() {
        return Cow::Borrowed(record);
    }
    if reason == "setup-exit" {
        let Ok(code) = argument.parse::<i32>() else {
            return Cow::Borrowed(record);
        };
        if code.to_string() != argument {
            return Cow::Borrowed(record);
        }
        return if language == Language::English {
            Cow::Borrowed(diagnostic)
        } else {
            Cow::Owned(formatted::helper_setup_exit(language, code))
        };
    }
    if argument.is_empty()
        && let Some((_, text)) = REASONS.iter().find(|(id, _)| *id == reason)
    {
        return Cow::Borrowed(if language == Language::English {
            diagnostic
        } else {
            text.in_language(language)
        });
    }
    Cow::Borrowed(record)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_records_translate_reasons_without_altering_english_diagnostics() {
        for &(id, text) in REASONS {
            let english = text.in_language(Language::English);
            // The standalone producer also keeps English exceptions for logs.
            assert!(
                include_str!("handoff.cs")
                    .contains(&format!("new OwnedFailure(\"{id}\", \"{english}\")"))
            );
            let record = format!("{HEADER}{id}\n\n{english}");
            assert_eq!(message(Language::English, &record), english);
            assert_eq!(
                message(Language::Japanese, &record),
                text.in_language(Language::Japanese)
            );
            assert_ne!(message(Language::Japanese, &record), english);
        }
        for code in [i32::MIN, 20, 3010, i32::MAX] {
            let english = format!("Setup exited with code {code}.");
            let record = format!("{HEADER}setup-exit\n{code}\n{english}");
            assert_eq!(message(Language::English, &record), english);
            assert_eq!(
                message(Language::Japanese, &record),
                format!("セットアップが終了コード{code}で終了しました。")
            );
        }
    }

    #[test]
    fn old_native_unknown_and_malformed_helper_records_remain_literal() {
        for record in [
            "",
            "Update signature verification failed.",
            "native {detail}\n日本語.png 0x80004005",
            "towavue-update-error-v2\nsignature\n\nfuture",
            "towavue-update-error-v1\nsignature",
            "towavue-update-error-v1\nsignature\n",
            "towavue-update-error-v1\nsignature\n\n",
            "towavue-update-error-v1\nsignature\nextra\ndiagnostic",
            "towavue-update-error-v1\nunknown\n\ndiagnostic",
            "towavue-update-error-v1\nsetup-exit\n2147483648\ndiagnostic",
            "towavue-update-error-v1\nsetup-exit\n+20\ndiagnostic",
            "towavue-update-error-v1\nsetup-exit\n020\ndiagnostic",
            "towavue-update-error-v1\nsetup-exit\n\ndiagnostic",
        ] {
            for language in [Language::English, Language::Japanese] {
                assert_eq!(message(language, record), record);
            }
        }
    }
}
