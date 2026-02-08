//! Platform-specific DPAPI encrypt/decrypt helpers.
//!
//! On Windows, uses CryptProtectData/CryptUnprotectData.
//! On other platforms, uses plaintext passthrough (dev only).

use crate::error::Result;

#[cfg(windows)]
use crate::error::CoreError;

#[cfg(windows)]
pub fn dpapi_encrypt(data: &[u8]) -> Result<Vec<u8>> {
    use windows::Win32::Foundation::*;
    use windows::Win32::Security::Cryptography::*;

    unsafe {
        let mut input_data = data.to_vec();
        let input = CRYPT_INTEGER_BLOB {
            cbData: input_data.len() as u32,
            pbData: input_data.as_mut_ptr(),
        };

        let mut output = CRYPT_INTEGER_BLOB::default();

        CryptProtectData(
            &input,
            None,
            None,
            None,
            None,
            Default::default(),
            &mut output,
        )
        .map_err(|e| CoreError::Crypto(format!("DPAPI encrypt failed: {}", e)))?;

        let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(output.pbData as *mut _));

        Ok(result)
    }
}

#[cfg(windows)]
pub fn dpapi_decrypt(data: &[u8]) -> Result<Vec<u8>> {
    use windows::Win32::Foundation::*;
    use windows::Win32::Security::Cryptography::*;

    unsafe {
        let mut input_data = data.to_vec();
        let input = CRYPT_INTEGER_BLOB {
            cbData: input_data.len() as u32,
            pbData: input_data.as_mut_ptr(),
        };

        let mut output = CRYPT_INTEGER_BLOB::default();

        CryptUnprotectData(
            &input,
            None,
            None,
            None,
            None,
            Default::default(),
            &mut output,
        )
        .map_err(|e| CoreError::Crypto(format!("DPAPI decrypt failed: {}", e)))?;

        let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(output.pbData as *mut _));

        Ok(result)
    }
}

#[cfg(not(windows))]
pub fn dpapi_encrypt(data: &[u8]) -> Result<Vec<u8>> {
    tracing::warn!("DPAPI not available - using plaintext storage (dev only)");
    Ok(data.to_vec())
}

#[cfg(not(windows))]
pub fn dpapi_decrypt(data: &[u8]) -> Result<Vec<u8>> {
    tracing::warn!("DPAPI not available - plaintext storage (dev only)");
    Ok(data.to_vec())
}
