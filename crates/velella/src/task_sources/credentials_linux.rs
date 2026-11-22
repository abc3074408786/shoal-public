//! Store only an authenticated ciphertext in SQLite; Secret Service owns the key.
use anyhow::{Context, Result};
use fs2::FileExt;
use ring::{aead, rand::{SecureRandom, SystemRandom}};
use std::{fs::OpenOptions, os::unix::fs::{OpenOptionsExt, PermissionsExt}, sync::Mutex, time::{Duration, Instant}};

const HEADER: &[u8] = b"VLSS1";
const SERVICE: &str = "dev.velella.task-sources";
const KEY_NAME: &str = "encryption-key-v1";
static KEY_ACCESS: Mutex<()> = Mutex::new(());

fn key(create: bool) -> Result<aead::LessSafeKey> {
    // Serialise key creation across threads and profiles of this Unix user.
    // Only the lock is stored here: key bytes never go to the filesystem.
    let _thread = KEY_ACCESS.lock().map_err(|_| anyhow::anyhow!("凭据密钥锁不可用"))?;
    let home = std::env::var_os("HOME").context("无法确定用户凭据目录")?;
    let root = std::path::PathBuf::from(home).join(".local/share/velella-credentials");
    std::fs::create_dir_all(&root)?;
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
    let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false)
        .mode(0o600).open(root.join("key.lock"))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match lock.try_lock_exclusive() {
            Ok(()) => break,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            Err(_) => anyhow::bail!("凭据密钥正在使用，请稍后重试"),
        }
    }
    let entry = keyring::Entry::new(SERVICE, KEY_NAME).map_err(|_| anyhow::anyhow!("无法连接系统密钥环"))?;
    let secret = match entry.get_secret() {
        Ok(secret) => secret,
        Err(keyring::Error::NoEntry) if create => {
            let mut secret = vec![0u8; 32];
            SystemRandom::new().fill(&mut secret).map_err(|_| anyhow::anyhow!("无法生成凭据密钥"))?;
            entry.set_secret(&secret).map_err(|_| anyhow::anyhow!("无法保存凭据密钥，请解锁系统密钥环后重试"))?;
            let saved = entry.get_secret().map_err(|_| anyhow::anyhow!("无法确认系统密钥环已保存密钥"))?;
            anyhow::ensure!(saved == secret, "系统密钥环保存校验失败");
            secret
        }
        Err(keyring::Error::NoEntry) => anyhow::bail!("系统密钥环中没有此凭据的密钥，请重新连接任务来源"),
        Err(_) => anyhow::bail!("无法读取系统密钥环，请确认 Secret Service 可用且已解锁"),
    };
    let unbound = aead::UnboundKey::new(&aead::AES_256_GCM, &secret).map_err(|_| anyhow::anyhow!("系统密钥环中的凭据密钥格式不正确"))?;
    Ok(aead::LessSafeKey::new(unbound))
}

pub(super) fn crypt(bytes: &[u8], protect: bool) -> Result<Vec<u8>> {
    if protect {
        let key = key(true)?;
        let mut nonce = [0u8; 12];
        SystemRandom::new().fill(&mut nonce).map_err(|_| anyhow::anyhow!("无法生成凭据随机数"))?;
        let mut ciphertext = bytes.to_vec();
        key.seal_in_place_append_tag(aead::Nonce::assume_unique_for_key(nonce), aead::Aad::from(HEADER), &mut ciphertext)
            .map_err(|_| anyhow::anyhow!("凭据加密失败"))?;
        Ok([HEADER, nonce.as_slice(), ciphertext.as_slice()].concat())
    } else {
        anyhow::ensure!(bytes.starts_with(HEADER) && bytes.len() >= HEADER.len() + 12 + aead::AES_256_GCM.tag_len(),
            "凭据格式不受支持，请在原平台使用或重新连接来源");
        let nonce: [u8; 12] = bytes[HEADER.len()..HEADER.len()+12].try_into()?;
        let mut ciphertext = bytes[HEADER.len()+12..].to_vec();
        let key = key(false)?;
        let plaintext = key.open_in_place(aead::Nonce::assume_unique_for_key(nonce), aead::Aad::from(HEADER), &mut ciphertext)
            .map_err(|_| anyhow::anyhow!("凭据无法解密或已损坏，请重新连接来源"))?;
        Ok(plaintext.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_secret_service_has_no_plaintext_fallback() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "task_sources::credentials::linux::tests::unavailable_service_fixture", "--ignored"])
            .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/tmp/velella-nonexistent-secret-service")
            .output().unwrap();
        assert!(output.status.success(), "missing Secret Service must fail closed");
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"), "fixture must actually run");
    }

    #[test]
    #[ignore = "subprocess fixture with an unavailable Secret Service"]
    fn unavailable_service_fixture() {
        assert!(crypt(b"fixture", true).is_err());
    }

    #[test]
    fn native_keyring_roundtrip_randomizes_and_rejects_tampering() {
        let plain = b"velella-test-fixture-not-a-real-token";
        let first = crypt(plain, true).unwrap();
        let second = crypt(plain, true).unwrap();
        assert_ne!(first, second);
        assert!(!first.windows(plain.len()).any(|v| v == plain));
        assert_eq!(crypt(&first, false).unwrap(), plain);
        let mut damaged = first;
        *damaged.last_mut().unwrap() ^= 1;
        assert!(crypt(&damaged, false).is_err());
        assert!(crypt(b"old-platform-blob", false).is_err());
        assert!(crypt(HEADER, false).is_err());
    }
}
