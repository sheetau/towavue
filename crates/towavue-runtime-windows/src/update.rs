//! Native update download, authentication and staging boundary.
mod crypto;
mod handoff;
mod http;
mod service;
mod storage;
pub use handoff::PendingHandoff;
pub use service::{UpdateEvent, UpdateService};
pub use storage::{CachedUpdate, StartupUpdate, UpdatePhase, UpdateStore};

use crate::Cancellation;
use std::{fs::File, io};
use towavue_core::release::{
    RELEASE_REPOSITORY, ReleaseManifest, ReleaseVersion, UPDATE_MANIFEST_NAME,
};

const PUBLIC_KEY_HEX: &str = include_str!("../../../packaging/windows/update-public-key.hex");

/// Constructible only after publisher authentication. Download callers may
/// inspect its metadata, but must verify the complete file before execution.
#[derive(Clone, Debug)]
pub struct SignedUpdate {
    manifest: ReleaseManifest,
    manifest_bytes: Vec<u8>,
    signature: Vec<u8>,
}

impl SignedUpdate {
    pub fn authenticate(manifest_bytes: &[u8], signature: &[u8]) -> io::Result<Self> {
        let manifest = ReleaseManifest::parse(manifest_bytes).map_err(io::Error::other)?;
        let text = PUBLIC_KEY_HEX.trim();
        if !text.len().is_multiple_of(2) || text.len() > 4096 {
            return Err(io::Error::other("Invalid embedded update public key"));
        }
        let key = (0..text.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&text[index..index + 2], 16).map_err(io::Error::other))
            .collect::<io::Result<Vec<_>>>()?;
        Self::authenticate_with_key(manifest, manifest_bytes, signature, &key)
    }

    fn authenticate_with_key(
        manifest: ReleaseManifest,
        manifest_bytes: &[u8],
        signature: &[u8],
        key: &[u8],
    ) -> io::Result<Self> {
        crypto::verify(key, manifest_bytes, signature)?;
        Ok(Self {
            manifest,
            manifest_bytes: manifest_bytes.to_vec(),
            signature: signature.to_vec(),
        })
    }

    pub fn manifest(&self) -> &ReleaseManifest {
        &self.manifest
    }

    pub fn signed_metadata(&self) -> (&[u8], &[u8]) {
        (&self.manifest_bytes, &self.signature)
    }

    /// Runs on a worker, into a newly reserved empty read/write staging file.
    /// Incomplete/error output must not be published or executed by the caller.
    pub fn download(&self, file: &mut File, cancel: &Cancellation) -> io::Result<()> {
        use std::io::{Seek, SeekFrom};
        if file.metadata()?.len() != 0 {
            return Err(io::Error::other(
                "Update download requires an empty staging file",
            ));
        }
        file.seek(SeekFrom::Start(0))?;
        http::get(
            &self.manifest.setup_url(),
            self.manifest.bytes,
            file,
            cancel,
        )?;
        file.sync_all()?;
        if cancel.is_cancelled() {
            return Err(io::ErrorKind::Interrupted.into());
        }
        self.verify_file(file)
    }

    /// The caller must keep the validated file protected from writes/replacement
    /// until launch. This method hashes the whole file from offset zero.
    pub fn verify_file(&self, file: &mut File) -> io::Result<()> {
        use std::io::{Seek, SeekFrom};
        if !file.metadata()?.is_file() || file.metadata()?.len() != self.manifest.bytes {
            return Err(io::Error::other(
                "Update file size does not match signed metadata",
            ));
        }
        file.seek(SeekFrom::Start(0))?;
        use std::io::Read;
        if crypto::sha256(file.take(self.manifest.bytes + 1))? != self.manifest.sha256 {
            return Err(io::Error::other(
                "Update file hash does not match signed metadata",
            ));
        }
        Ok(())
    }
}

/// Blocking network work: call from an update worker, never the event thread.
/// GitHub's latest-download route excludes drafts and prereleases. Each version's
/// signature and installer use immutable version-specific paths rather than a
/// second latest lookup, so a release change during download fails safely.
pub fn check_for_update(
    installed: ReleaseVersion,
    cancel: &Cancellation,
) -> io::Result<Option<SignedUpdate>> {
    let mut bytes = Vec::new();
    let url = format!("{RELEASE_REPOSITORY}/releases/latest/download/{UPDATE_MANIFEST_NAME}");
    if let Err(error) = http::get(&url, 256, &mut bytes, cancel) {
        return if error.kind() == io::ErrorKind::NotFound {
            Ok(None)
        } else {
            Err(error)
        };
    }
    let manifest = ReleaseManifest::parse(&bytes).map_err(io::Error::other)?;
    if !manifest.newer_than(installed) {
        return Ok(None);
    }
    let mut signature = Vec::new();
    http::get(&manifest.signature_url(), 512, &mut signature, cancel)?;
    SignedUpdate::authenticate(&bytes, &signature).map(Some)
}
