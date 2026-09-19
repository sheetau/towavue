//! Blocking WinHTTP operations run only on the update worker. Handles stay on
//! that thread; cancellation is checked between calls with finite timeouts.
use crate::Cancellation;
use std::{
    ffi::c_void,
    io::{self, Write},
    ptr,
    time::{Duration, Instant},
};
use windows::{
    Win32::Networking::WinHttp::*,
    core::{PCWSTR, w},
};

struct Handle(*mut c_void);

impl Handle {
    fn new(raw: *mut c_void) -> io::Result<Self> {
        if raw.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(raw))
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: each handle is owned once. Request is dropped before connection,
        // and connection before session. No asynchronous callbacks are registered.
        let _ = unsafe { WinHttpCloseHandle(self.0) };
    }
}

fn windows_error(value: windows::core::Error) -> io::Error {
    io::Error::other(value.to_string())
}

fn parts(url: &str) -> io::Result<(&str, &str)> {
    if url.len() > 8192
        || url
            .bytes()
            .any(|byte| byte <= 32 || byte == 127 || byte == b'\\' || byte == b'#')
    {
        return Err(io::Error::other("Invalid update URL"));
    }
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| io::Error::other("Update requires HTTPS"))?;
    let split = rest
        .find('/')
        .ok_or_else(|| io::Error::other("Update URL has no path"))?;
    let (host, path) = rest.split_at(split);
    if !matches!(
        host,
        "github.com" | "release-assets.githubusercontent.com" | "objects.githubusercontent.com"
    ) {
        return Err(io::Error::other(
            "Update redirect is outside GitHub release storage",
        ));
    }
    Ok((host, path))
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

fn check(cancel: &Cancellation, start: Instant) -> io::Result<()> {
    if cancel.is_cancelled() {
        return Err(io::ErrorKind::Interrupted.into());
    }
    if start.elapsed() > Duration::from_secs(600) {
        return Err(io::ErrorKind::TimedOut.into());
    }
    Ok(())
}

pub(super) fn get(
    url: &str,
    maximum: u64,
    output: &mut impl Write,
    cancel: &Cancellation,
) -> io::Result<u64> {
    parts(url)?;
    let start = Instant::now();
    check(cancel, start)?;
    // SAFETY: static UTF-16 agent and null proxy names. A synchronous session is
    // confined to this worker; system/user proxy discovery uses Windows settings.
    let session = Handle::new(unsafe {
        WinHttpOpen(
            w!("towavue-updater/1"),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        )
    })?;
    // SAFETY: owned live session, finite millisecond timeouts and native u32 data.
    unsafe {
        WinHttpSetTimeouts(session.0, 5000, 10000, 10000, 10000).map_err(windows_error)?;
        let protocols = WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2 | WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3;
        WinHttpSetOption(
            Some(session.0.cast_const()),
            WINHTTP_OPTION_SECURE_PROTOCOLS,
            Some(&protocols.to_ne_bytes()),
        )
        .map_err(windows_error)?;
    }
    let mut next = url.to_owned();
    for _ in 0..6 {
        check(cancel, start)?;
        let (host, path) = parts(&next)?;
        let host = wide(host);
        let path = wide(path);
        // SAFETY: session outlives connection, owned NUL-terminated names remain
        // alive across synchronous creation. HTTPS port is fixed; no credentials.
        let connection =
            Handle::new(unsafe { WinHttpConnect(session.0, PCWSTR(host.as_ptr()), 443, 0) })?;
        // SAFETY: connection outlives request and GET path remains live here.
        let request = Handle::new(unsafe {
            WinHttpOpenRequest(
                connection.0,
                w!("GET"),
                PCWSTR(path.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                ptr::null(),
                WINHTTP_FLAG_SECURE,
            )
        })?;
        // SAFETY: bounded native buffers and synchronous calls on owned handles.
        let status = unsafe {
            WinHttpSetOption(
                Some(request.0.cast_const()),
                WINHTTP_OPTION_REDIRECT_POLICY,
                Some(&WINHTTP_OPTION_REDIRECT_POLICY_NEVER.to_ne_bytes()),
            )
            .map_err(windows_error)?;
            WinHttpSendRequest(request.0, None, None, 0, 0, 0).map_err(windows_error)?;
            WinHttpReceiveResponse(request.0, ptr::null_mut()).map_err(windows_error)?;
            let mut status = 0u32;
            let mut size = 4u32;
            WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                PCWSTR::null(),
                Some((&raw mut status).cast()),
                &mut size,
                ptr::null_mut(),
            )
            .map_err(windows_error)?;
            status
        };
        if matches!(status, 301 | 302 | 303 | 307 | 308) {
            let mut location = [0u16; 8193];
            let mut bytes = (location.len() * 2) as u32;
            // SAFETY: size is in bytes and matches the allocated UTF-16 buffer.
            unsafe {
                WinHttpQueryHeaders(
                    request.0,
                    WINHTTP_QUERY_LOCATION,
                    PCWSTR::null(),
                    Some(location.as_mut_ptr().cast()),
                    &mut bytes,
                    ptr::null_mut(),
                )
            }
            .map_err(windows_error)?;
            let length = location
                .iter()
                .position(|unit| *unit == 0)
                .ok_or_else(|| io::Error::other("Unterminated redirect"))?;
            let location = String::from_utf16(&location[..length]).map_err(io::Error::other)?;
            next = if location.starts_with('/') && !location.starts_with("//") {
                format!("https://{}{location}", parts(&next)?.0)
            } else {
                location
            };
            parts(&next)?;
            continue;
        }
        if status == 404 {
            return Err(io::ErrorKind::NotFound.into());
        }
        if status != 200 {
            return Err(io::Error::other(format!(
                "Update server returned HTTP {status}"
            )));
        }
        let mut total = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            check(cancel, start)?;
            let mut count = 0u32;
            // SAFETY: Windows may write at most the supplied buffer length.
            unsafe {
                WinHttpReadData(
                    request.0,
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    &mut count,
                )
            }
            .map_err(windows_error)?;
            if count == 0 {
                return Ok(total);
            }
            total += u64::from(count);
            if total > maximum {
                return Err(io::Error::other("Update response exceeds its size limit"));
            }
            output.write_all(&buffer[..count as usize])?;
        }
    }
    Err(io::Error::other("Too many update redirects"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_validation_refuses_downgrades_credentials_ports_and_foreign_hosts() {
        for url in [
            "http://github.com/file",
            "https://github.com.evil.test/file",
            "https://github.com@evil.test/file",
            "https://user@github.com/file",
            "https://github.com:443/file",
            "https://github.com\\evil/file",
            "https://github.com/file\r\nHeader:x",
            "https://evil.test/file",
            "https://github.com/file#fragment",
            "https://github.com/file\0",
        ] {
            assert!(parts(url).is_err(), "{url:?}");
        }
        for url in [
            "https://github.com/sheetau/towavue/releases/latest/download/towavue-update-v1.txt",
            "https://release-assets.githubusercontent.com/github-production-release-asset/a?key=value&b=c",
        ] {
            assert!(parts(url).is_ok());
        }
        let cancel = Cancellation::default();
        cancel.cancel();
        assert_eq!(
            get("https://github.com/file", 1, &mut Vec::new(), &cancel)
                .expect_err("canceled before network")
                .kind(),
            io::ErrorKind::Interrupted
        );
    }
}
