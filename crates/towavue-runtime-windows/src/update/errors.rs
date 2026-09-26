//! Retain I/O causes across cloned worker events; Display remains English.
use std::{fmt, io, sync::Arc};
use towavue_core::localization::{Language, Text, formatted};

#[derive(Clone, Debug)]
pub struct UpdateError(Arc<io::Error>);

impl UpdateError {
    pub fn message(&self, language: Language) -> String {
        if let Some(cause) = self.0.get_ref() {
            if let Some(cause) = cause.downcast_ref::<Failure>() {
                return cause.message(language);
            }
            if cause.is::<towavue_core::release::ReleaseMetadataError>() {
                return Text::UpdateMetadataInvalid.in_language(language).into();
            }
        }
        // External OS, encoding and helper diagnostics are literal details.
        self.to_string()
    }
}

impl From<io::Error> for UpdateError {
    fn from(error: io::Error) -> Self {
        Self(Arc::new(error))
    }
}

impl From<String> for UpdateError {
    fn from(error: String) -> Self {
        io::Error::other(error).into()
    }
}

impl From<&str> for UpdateError {
    fn from(error: &str) -> Self {
        error.to_owned().into()
    }
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for UpdateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref())
    }
}

#[derive(Debug)]
enum Failure {
    Text(Text),
    Http(u32),
    HelperExited { status: String, detail: String },
}

impl Failure {
    fn message(&self, language: Language) -> String {
        match self {
            Self::Text(text) => text.in_language(language).into(),
            Self::Http(status) => formatted::update_http_status(language, *status),
            Self::HelperExited { status, detail } => {
                formatted::update_helper_exited(language, status, detail)
            }
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message(Language::English))
    }
}

impl std::error::Error for Failure {}

pub(super) fn text(text: Text) -> io::Error {
    with_kind(io::ErrorKind::Other, text)
}

pub(super) fn with_kind(kind: io::ErrorKind, text: Text) -> io::Error {
    io::Error::new(kind, Failure::Text(text))
}

pub(super) fn http_status(status: u32) -> io::Error {
    io::Error::other(Failure::Http(status))
}

pub(super) fn helper_exited(status: String, detail: String) -> io::Error {
    io::Error::other(Failure::HelperExited { status, detail })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_update_error_preserves_kind_diagnostics_and_cause_across_cloned_events() {
        let error = with_kind(io::ErrorKind::TimedOut, Text::UpdateHelperTimeout);
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(error.to_string(), "Update helper did not become ready");
        let event = super::super::UpdateEvent::Error {
            message: error.into(),
            startup: false,
            operation: Some(41),
        };
        let (send, receive) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || send.send(event.clone()).expect("send"));
        let super::super::UpdateEvent::Error {
            message,
            startup,
            operation,
        } = receive.recv().expect("receive")
        else {
            panic!("error event")
        };
        worker.join().expect("worker");
        assert!(!startup);
        assert_eq!(operation, Some(41));
        assert_eq!(message.0.kind(), io::ErrorKind::TimedOut);
        assert_eq!(message.message(Language::English), message.to_string());
        assert_eq!(
            message.message(Language::Japanese),
            "更新用の補助プロセスの準備が時間内に完了しませんでした"
        );
        let cause = std::error::Error::source(&message).expect("original cause");
        assert_eq!(
            cause.downcast_ref::<io::Error>().expect("I/O cause").kind(),
            io::ErrorKind::TimedOut
        );
    }

    #[test]
    fn update_details_and_native_errors_remain_literal_while_owned_explanations_translate() {
        let detail = "native {detail}\n日本語.png 0x80004005";
        for source in [io::Error::from_raw_os_error(5), io::Error::other(detail)] {
            let expected = source.to_string();
            let error = UpdateError::from(source);
            assert_eq!(error.message(Language::Japanese), expected);
        }
        let error = UpdateError::from(helper_exited("exit code: 7".into(), detail.into()));
        assert_eq!(
            error.to_string(),
            format!("Update helper exited before readiness (exit code: 7): {detail}")
        );
        assert_eq!(
            error.message(Language::Japanese),
            format!("更新用の補助プロセスが準備完了前に終了しました（exit code: 7）: {detail}")
        );
        let error = UpdateError::from(http_status(503));
        assert_eq!(error.to_string(), "Update server returned HTTP 503");
        assert_eq!(
            error.message(Language::Japanese),
            "更新サーバーがHTTP 503を返しました"
        );
        let error = UpdateError::from(
            super::super::SignedUpdate::authenticate(b"invalid", b"invalid")
                .expect_err("reject malformed signed metadata"),
        );
        assert_eq!(error.to_string(), "invalid or unsupported update metadata");
        assert_eq!(
            error.message(Language::Japanese),
            "更新情報が不正か、対応していない形式です"
        );
    }
}
