//! Strict release metadata shared by the updater and release verification.
//! Parsing does not authenticate a manifest: native signature verification must
//! succeed before its payload may be downloaded or executed.
use std::{fmt, str::FromStr};

pub const RELEASE_REPOSITORY: &str = "https://github.com/sheetau/towavue";
pub const UPDATE_MANIFEST_NAME: &str = "towavue-update-v1.txt";
pub const UPDATE_SIGNATURE_NAME: &str = "towavue-update-v1.sig";
pub const MAX_SETUP_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReleaseVersion(pub u32, pub u32, pub u32);

impl FromStr for ReleaseVersion {
    type Err = ReleaseMetadataError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut parts = text.split('.');
        let mut component = || {
            let value = parts.next().ok_or(ReleaseMetadataError)?;
            if value.is_empty()
                || (value.len() > 1 && value.starts_with('0'))
                || !value.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(ReleaseMetadataError);
            }
            value.parse::<u32>().map_err(|_| ReleaseMetadataError)
        };
        let version = Self(component()?, component()?, component()?);
        if parts.next().is_some() {
            return Err(ReleaseMetadataError);
        }
        Ok(version)
    }
}

impl fmt::Display for ReleaseVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseManifest {
    pub version: ReleaseVersion,
    pub bytes: u64,
    pub sha256: [u8; 32],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReleaseMetadataError;

impl fmt::Display for ReleaseMetadataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid or unsupported update metadata")
    }
}

impl std::error::Error for ReleaseMetadataError {}

impl ReleaseManifest {
    /// Canonical, bounded UTF-8/LF metadata. No URL, path or command is accepted
    /// from the network: all download paths derive from a strict stable version.
    pub fn parse(bytes: &[u8]) -> Result<Self, ReleaseMetadataError> {
        if bytes.len() > 256 {
            return Err(ReleaseMetadataError);
        }
        let text = std::str::from_utf8(bytes).map_err(|_| ReleaseMetadataError)?;
        let fields: Vec<_> = text.split('\n').collect();
        let [
            "towavue-update-v1",
            version,
            "windows-x64",
            length,
            digest,
            "",
        ] = fields.as_slice()
        else {
            return Err(ReleaseMetadataError);
        };
        let version = version.parse()?;
        let length_number = length.parse::<u64>().map_err(|_| ReleaseMetadataError)?;
        if !(1..=MAX_SETUP_BYTES).contains(&length_number)
            || length_number.to_string() != *length
            || digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(ReleaseMetadataError);
        }
        let mut sha256 = [0; 32];
        for (index, byte) in sha256.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&digest[index * 2..index * 2 + 2], 16)
                .map_err(|_| ReleaseMetadataError)?;
        }
        Ok(Self {
            version,
            bytes: length_number,
            sha256,
        })
    }

    pub fn asset_name(&self) -> String {
        format!("towavue-{}-windows-x64-setup.exe", self.version)
    }

    pub fn setup_url(&self) -> String {
        format!(
            "{RELEASE_REPOSITORY}/releases/download/v{}/{}",
            self.version,
            self.asset_name()
        )
    }

    pub fn signature_url(&self) -> String {
        format!(
            "{RELEASE_REPOSITORY}/releases/download/v{}/{UPDATE_SIGNATURE_NAME}",
            self.version
        )
    }

    pub fn newer_than(&self, installed: ReleaseVersion) -> bool {
        self.version > installed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(version: &str, size: &str, hash: &str) -> Vec<u8> {
        format!("towavue-update-v1\n{version}\nwindows-x64\n{size}\n{hash}\n").into_bytes()
    }

    #[test]
    fn strict_stable_versions_compare_numerically_and_refuse_other_channels() {
        assert!("1.10.0".parse::<ReleaseVersion>().expect("stable") > ReleaseVersion(1, 9, 99));
        for value in [
            "",
            "1.2",
            "1.2.3.4",
            "01.2.3",
            "1.2.3-beta",
            "v1.2.3",
            "1.2.3+build",
            "+1.2.3",
            "1.2.3\r",
            "4294967296.0.0",
            "../1.2.3",
        ] {
            assert!(value.parse::<ReleaseVersion>().is_err(), "{value:?}");
        }
    }

    #[test]
    fn metadata_cannot_supply_commands_paths_channels_or_unbounded_downloads() {
        let hash = "ab".repeat(32);
        let good = manifest("1.2.3", "123456", &hash);
        let parsed = ReleaseManifest::parse(&good).expect("canonical manifest");
        assert_eq!(parsed.sha256, [0xab; 32]);
        assert_eq!(
            parsed.setup_url(),
            "https://github.com/sheetau/towavue/releases/download/v1.2.3/towavue-1.2.3-windows-x64-setup.exe"
        );
        assert!(parsed.newer_than(ReleaseVersion(1, 2, 2)));
        assert!(!parsed.newer_than(ReleaseVersion(1, 2, 3)));
        assert!(!parsed.newer_than(ReleaseVersion(2, 0, 0)));
        for bad in [
            manifest("1.2.3", "0", &hash),
            manifest("1.2.3", "536870913", &hash),
            manifest("1.2.3", "0123", &hash),
            manifest("1.2.3", "123", &hash.to_uppercase()),
            manifest("1.2.3", "123", &"x".repeat(64)),
            manifest("1.2.3-beta", "123", &hash),
        ] {
            assert!(ReleaseManifest::parse(&bad).is_err());
        }
        for bad in [
            String::from_utf8(good.clone())
                .expect("utf8")
                .replace('\n', "\r\n"),
            String::from_utf8(good.clone())
                .expect("utf8")
                .replace("windows-x64", "windows-arm64"),
            format!("{}extra\n", String::from_utf8(good.clone()).expect("utf8")),
        ] {
            assert!(ReleaseManifest::parse(bad.as_bytes()).is_err());
        }
        assert!(ReleaseManifest::parse(&good[..good.len() - 1]).is_err());
        assert!(ReleaseManifest::parse(&[b'a'; 257]).is_err());
    }
}
