//! DPAPI wrapper for the SQLCipher DEK (D-007, spec §5): `CryptProtectData`
//! in user scope, so the DEK is undecryptable on another Windows account and
//! after an admin password reset (orphan recovery per spec §11).

#[derive(Debug, thiserror::Error)]
#[error("DPAPI failure: {0}")]
pub struct DpapiError(pub String);

#[cfg(windows)]
pub use windows_impl::{protect, unprotect};

#[cfg(windows)]
mod windows_impl {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    use super::DpapiError;

    const FLAGS: u32 = CRYPTPROTECT_UI_FORBIDDEN;

    fn e(err: windows::core::Error) -> DpapiError {
        DpapiError(err.to_string())
    }

    pub fn protect(data: &[u8]) -> Result<Vec<u8>, DpapiError> {
        unsafe {
            let input = CRYPT_INTEGER_BLOB {
                cbData: data.len() as u32,
                pbData: data.as_ptr() as *mut u8,
            };
            let mut out = CRYPT_INTEGER_BLOB::default();
            CryptProtectData(&input, PCWSTR::null(), None, None, None, FLAGS, &mut out)
                .map_err(e)?;
            // The API allocates the output; copy then release with LocalFree.
            let wrapped = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
            let _ = LocalFree(Some(HLOCAL(out.pbData.cast())));
            Ok(wrapped)
        }
    }

    pub fn unprotect(wrapped: &[u8]) -> Result<Vec<u8>, DpapiError> {
        unsafe {
            let input = CRYPT_INTEGER_BLOB {
                cbData: wrapped.len() as u32,
                pbData: wrapped.as_ptr() as *mut u8,
            };
            let mut out = CRYPT_INTEGER_BLOB::default();
            CryptUnprotectData(&input, None, None, None, None, FLAGS, &mut out).map_err(e)?;
            let plain = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
            let _ = LocalFree(Some(HLOCAL(out.pbData.cast())));
            Ok(plain)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DPAPI user-scope round trip: what is encrypted by this Windows user
    /// decrypts identically (the cross-user/cross-machine negative case is
    /// asserted indirectly by FR-33's different-account criterion).
    #[test]
    fn protect_unprotect_round_trip() {
        let dek: [u8; 32] = core::array::from_fn(|i| (i * 7 + 3) as u8);
        let wrapped = protect(&dek).expect("protect");
        assert_ne!(wrapped, dek, "DPAPI output must differ from input");
        let plain = unprotect(&wrapped).expect("unprotect");
        assert_eq!(plain, dek);
    }
}
