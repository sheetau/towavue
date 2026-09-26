//! Keep worker failures typed until the receiving window chooses its language.
use std::{fmt, sync::Arc};

use towavue_core::localization::Language;

use crate::{AudioOutputError, DecodeError};

#[derive(Debug)]
enum Cause {
    Decode(DecodeError),
    Audio(AudioOutputError),
    Detail,
}

/// Cloneable error payload shared by decode and audio notifications.
#[derive(Clone, Debug)]
pub struct PlaybackFailure {
    cause: Arc<Cause>,
    diagnostic: String,
}

impl PlaybackFailure {
    pub fn message(&self, language: Language) -> String {
        match self.cause.as_ref() {
            Cause::Decode(error) => error.message(language),
            Cause::Audio(error) => error.message(language),
            Cause::Detail => self.diagnostic.clone(),
        }
    }
}

impl fmt::Display for PlaybackFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.diagnostic)
    }
}

impl std::error::Error for PlaybackFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self.cause.as_ref() {
            Cause::Decode(error) => Some(error),
            Cause::Audio(error) => Some(error),
            Cause::Detail => None,
        }
    }
}

// Events previously compared String payloads. Preserve diagnostic equality,
// independent of the receiving window's language or the cause's Arc identity.
impl PartialEq for PlaybackFailure {
    fn eq(&self, other: &Self) -> bool {
        self.diagnostic == other.diagnostic
    }
}

impl Eq for PlaybackFailure {}

impl From<DecodeError> for PlaybackFailure {
    fn from(error: DecodeError) -> Self {
        Self {
            diagnostic: error.to_string(),
            cause: Arc::new(Cause::Decode(error)),
        }
    }
}

impl From<AudioOutputError> for PlaybackFailure {
    fn from(error: AudioOutputError) -> Self {
        Self {
            diagnostic: error.to_string(),
            cause: Arc::new(Cause::Audio(error)),
        }
    }
}

impl From<String> for PlaybackFailure {
    fn from(diagnostic: String) -> Self {
        Self {
            diagnostic,
            cause: Arc::new(Cause::Detail),
        }
    }
}

impl From<&str> for PlaybackFailure {
    fn from(diagnostic: &str) -> Self {
        diagnostic.to_owned().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlaybackError;
    use std::error::Error;

    #[test]
    fn worker_clone_keeps_typed_cause_and_diagnostic_equality() {
        let failure = PlaybackFailure::from(DecodeError::WorkerStart(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "native {detail} 日本語.mp4",
        )));
        let (tx, rx) = std::sync::mpsc::channel();
        let sent = failure.clone();
        std::thread::spawn(move || tx.send(sent).expect("send worker failure"))
            .join()
            .expect("join worker");
        let received = rx.recv().expect("receive worker failure");
        assert_eq!(received, failure);
        assert_eq!(received, failure.to_string().into());
        assert_eq!(received.message(Language::English), received.to_string());
        assert_eq!(
            received.message(Language::Japanese),
            "メディアの読み込み処理を開始できません: native {detail} 日本語.mp4"
        );
        assert!(
            matches!(received.source().expect("decode cause").downcast_ref::<DecodeError>(),
            Some(DecodeError::WorkerStart(error)) if error.kind() == std::io::ErrorKind::PermissionDenied)
        );
        let literal: PlaybackFailure = "native {detail} 日本語.mp4".into();
        assert_eq!(literal.message(Language::Japanese), literal.to_string());
        assert!(literal.source().is_none());
    }

    #[test]
    fn audio_and_playback_messages_preserve_english_and_translate_nested_causes() {
        for error in [
            AudioOutputError::Tempo(ffmpeg_next::Error::InvalidData),
            AudioOutputError::UnsupportedFormat(44100, 6),
            AudioOutputError::Wasapi("native {detail} 日本語 0x80004005".into()),
            AudioOutputError::Closed,
            AudioOutputError::EndpointChanged,
            AudioOutputError::ZeroClockFrequency,
        ] {
            let failure = PlaybackFailure::from(error.clone());
            assert_eq!(error.message(Language::English), error.to_string());
            assert_eq!(
                failure.message(Language::Japanese),
                error.message(Language::Japanese)
            );
            assert_ne!(failure.message(Language::Japanese), failure.to_string());
            assert!(
                failure
                    .source()
                    .expect("audio cause")
                    .is::<AudioOutputError>()
            );
        }
        for error in [
            PlaybackError::InvalidSelection,
            PlaybackError::Probe(DecodeError::NoMediaStream),
            PlaybackError::Audio(AudioOutputError::Closed),
            PlaybackError::Thread(std::io::Error::other("native {detail} 日本語")),
        ] {
            assert_eq!(error.message(Language::English), error.to_string());
            assert_ne!(error.message(Language::Japanese), error.to_string());
        }
        assert_eq!(
            PlaybackError::Audio(AudioOutputError::Closed).message(Language::Japanese),
            "音声出力に失敗しました: 音声出力の処理が停止しました"
        );
    }
}
