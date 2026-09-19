use std::io::{self, Read};
use windows::Win32::Security::Cryptography::*;

fn error(value: windows::core::Error) -> io::Error {
    io::Error::other(value.to_string())
}

struct Hash(BCRYPT_HASH_HANDLE);

impl Drop for Hash {
    fn drop(&mut self) {
        // SAFETY: this object owns the hash; no other thread or pointer retains it.
        let _ = unsafe { BCryptDestroyHash(self.0) };
    }
}

struct Key(BCRYPT_KEY_HANDLE);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: this object exclusively owns the imported/generated key.
        let _ = unsafe { BCryptDestroyKey(self.0) };
    }
}

pub(super) fn sha256(mut reader: impl Read) -> io::Result<[u8; 32]> {
    let mut handle = BCRYPT_HASH_HANDLE::default();
    // SAFETY: CNG allocates the hash object; its RAII owner releases it. The
    // algorithm pseudo-handle is process-constant and must not be closed.
    unsafe { BCryptCreateHash(BCRYPT_SHA256_ALG_HANDLE, &mut handle, None, None, 0) }
        .ok()
        .map_err(error)?;
    let hash = Hash(handle);
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        // SAFETY: CNG reads only this borrowed slice during the synchronous call.
        unsafe { BCryptHashData(hash.0, &buffer[..count], 0) }
            .ok()
            .map_err(error)?;
    }
    let mut digest = [0; 32];
    // SAFETY: SHA-256 writes exactly the provided 32-byte output buffer.
    unsafe { BCryptFinishHash(hash.0, &mut digest, 0) }
        .ok()
        .map_err(error)?;
    Ok(digest)
}

pub(super) fn verify(public_blob: &[u8], manifest: &[u8], signature: &[u8]) -> io::Result<()> {
    // Bound all native input even if a future caller omits transport limits.
    if public_blob.len() > 2048 || manifest.len() > 256 || signature.len() != 512 {
        return Err(io::Error::other("Invalid update signature size"));
    }
    let mut handle = BCRYPT_KEY_HANDLE::default();
    // SAFETY: CNG copies the bounded public blob; Key owns the result and is
    // destroyed before this function returns. No private key is imported.
    unsafe {
        BCryptImportKeyPair(
            BCRYPT_RSA_ALG_HANDLE,
            None,
            BCRYPT_RSAPUBLIC_BLOB,
            &mut handle,
            public_blob,
            0,
        )
    }
    .ok()
    .map_err(error)?;
    let key = Key(handle);
    let padding = BCRYPT_PKCS1_PADDING_INFO {
        pszAlgId: BCRYPT_SHA256_ALGORITHM,
    };
    let digest = sha256(manifest)?;
    // SAFETY: every pointer refers to a live native structure or borrowed slice
    // for this synchronous verification. RSA-4096/PKCS1/SHA256 matches publishing.
    unsafe {
        BCryptVerifySignature(
            key.0,
            Some((&raw const padding).cast()),
            &digest,
            signature,
            BCRYPT_PAD_PKCS1,
        )
    }
    .ok()
    .map_err(|_| io::Error::other("Update signature verification failed"))
}

#[cfg(test)]
pub(super) struct TestSigner {
    key: Key,
    pub public: Vec<u8>,
}

#[cfg(test)]
impl TestSigner {
    pub fn new() -> Self {
        // SAFETY: only the public half is exported. The private key is ephemeral,
        // exclusively owned by Key, and never written to a file or the key store.
        unsafe {
            let mut handle = BCRYPT_KEY_HANDLE::default();
            BCryptGenerateKeyPair(BCRYPT_RSA_ALG_HANDLE, &mut handle, 4096, 0)
                .ok()
                .expect("generate test key");
            let key = Key(handle);
            BCryptFinalizeKeyPair(key.0, 0)
                .ok()
                .expect("finalize test key");
            let mut count = 0;
            BCryptExportKey(key.0, None, BCRYPT_RSAPUBLIC_BLOB, None, &mut count, 0)
                .ok()
                .expect("public size");
            let mut public = vec![0; count as usize];
            BCryptExportKey(
                key.0,
                None,
                BCRYPT_RSAPUBLIC_BLOB,
                Some(&mut public),
                &mut count,
                0,
            )
            .ok()
            .expect("public key");
            Self { key, public }
        }
    }

    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        let digest = sha256(message).expect("digest");
        let padding = BCRYPT_PKCS1_PADDING_INFO {
            pszAlgId: BCRYPT_SHA256_ALGORITHM,
        };
        let mut signature = vec![0; 512];
        let mut count = 0;
        // SAFETY: the test key, digest, padding and output live for this call.
        unsafe {
            BCryptSignHash(
                self.key.0,
                Some((&raw const padding).cast()),
                &digest,
                Some(&mut signature),
                &mut count,
                BCRYPT_PAD_PKCS1,
            )
        }
        .ok()
        .expect("sign fixture");
        assert_eq!(count, 512);
        signature
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_streams_and_signed_manifest_rejects_tampering_and_wrong_keys() {
        assert_eq!(
            sha256(&b"abc"[..]).expect("hash"),
            [
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
                0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
                0xf2, 0x00, 0x15, 0xad
            ]
        );
        let data = vec![42; 200_000];
        assert_eq!(
            sha256(data.as_slice()).expect("stream"),
            sha256(std::io::Cursor::new(data)).expect("cursor")
        );
        // SAFETY: test-only key generation; private key stays in CNG memory,
        // is never persisted/exported and is released by Key.
        unsafe {
            let mut handle = BCRYPT_KEY_HANDLE::default();
            BCryptGenerateKeyPair(BCRYPT_RSA_ALG_HANDLE, &mut handle, 4096, 0)
                .ok()
                .expect("generate");
            let key = Key(handle);
            BCryptFinalizeKeyPair(key.0, 0).ok().expect("finalize");
            let mut count = 0;
            BCryptExportKey(key.0, None, BCRYPT_RSAPUBLIC_BLOB, None, &mut count, 0)
                .ok()
                .expect("public size");
            let mut public = vec![0; count as usize];
            BCryptExportKey(
                key.0,
                None,
                BCRYPT_RSAPUBLIC_BLOB,
                Some(&mut public),
                &mut count,
                0,
            )
            .ok()
            .expect("public key");
            let message = b"towavue-update-v1\n1.0.1\nwindows-x64\n1\n";
            let digest = sha256(&message[..]).expect("digest");
            let padding = BCRYPT_PKCS1_PADDING_INFO {
                pszAlgId: BCRYPT_SHA256_ALGORITHM,
            };
            let mut signature = vec![0; 512];
            BCryptSignHash(
                key.0,
                Some((&raw const padding).cast()),
                &digest,
                Some(&mut signature),
                &mut count,
                BCRYPT_PAD_PKCS1,
            )
            .ok()
            .expect("sign");
            verify(&public, message, &signature).expect("matching signature");
            assert!(verify(&public, b"changed", &signature).is_err());
            signature[0] ^= 1;
            assert!(verify(&public, message, &signature).is_err());
            signature[0] ^= 1;
            assert!(verify(&public, message, &signature[..511]).is_err());
            public[40] ^= 1;
            assert!(verify(&public, message, &signature).is_err());
        }
    }
}
