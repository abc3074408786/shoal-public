use anyhow::{Context, Result};
use base64::Engine;

#[cfg(windows)]
fn crypt(bytes: &[u8], protect: bool) -> Result<Vec<u8>> {
    use windows_sys::Win32::{Foundation::LocalFree, Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN}};
    let input=CRYPT_INTEGER_BLOB{cbData:bytes.len().try_into()?,pbData:bytes.as_ptr() as *mut u8};
    let mut output=CRYPT_INTEGER_BLOB{cbData:0,pbData:std::ptr::null_mut()};
    unsafe {
        let ok=if protect {CryptProtectData(&input,std::ptr::null(),std::ptr::null(),std::ptr::null_mut(),std::ptr::null(),CRYPTPROTECT_UI_FORBIDDEN,&mut output)}
        else {CryptUnprotectData(&input,std::ptr::null_mut(),std::ptr::null(),std::ptr::null_mut(),std::ptr::null(),CRYPTPROTECT_UI_FORBIDDEN,&mut output)};
        anyhow::ensure!(ok!=0,"Windows 凭据保护失败，请在原 Windows 用户下重新连接来源");
        let result=std::slice::from_raw_parts(output.pbData,output.cbData as usize).to_vec();
        LocalFree(output.pbData as _);
        Ok(result)
    }
}
#[cfg(target_os = "linux")]
#[path = "credentials_linux.rs"]
mod linux;
#[cfg(target_os = "linux")]
fn crypt(bytes: &[u8], protect: bool) -> Result<Vec<u8>> { linux::crypt(bytes, protect) }

#[cfg(not(any(windows, target_os = "linux")))]
fn crypt(_bytes: &[u8], _protect: bool) -> Result<Vec<u8>> {anyhow::bail!("此平台尚未提供系统凭据保护")}

pub fn protect(token: &str) -> Result<String> {
    if token.is_empty(){return Ok(String::new())}
    Ok(base64::engine::general_purpose::STANDARD.encode(crypt(token.as_bytes(),true)?))
}
pub fn unprotect(secret: &str) -> Result<String> {
    if secret.is_empty(){return Ok(String::new())}
    let bytes=base64::engine::general_purpose::STANDARD.decode(secret).context("凭据记录无法读取")?;
    String::from_utf8(crypt(&bytes,false)?).context("凭据记录无法解密")
}
