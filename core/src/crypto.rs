//! OpenPGP (rPGP, pure Rust): keys, PGP/MIME signing and encryption
//! (RFC 3156), decryption, signature checks and Autocrypt key exchange.
//!
//! Key storage, in the user's data directory (owner-only permissions):
//! - `keys/own/<address>.asc`: the account's secret key (armored);
//! - `keys/contacts/<address>.asc`: public keys of correspondents imported
//!   by hand (trusted: a signature by one of them is "valid");
//! - `keys/autocrypt/<address>.asc`: keys collected from Autocrypt headers.
//!   Anyone can put any key in a header with any From address, so these
//!   are only used to encrypt and a signature by one is "unverified"; a
//!   key already held is never replaced by one from a header.

use anyhow::{Context, Result, anyhow, bail};
use pgp::composed::{
    ArmorOptions, Deserializable, DetachedSignature, EncryptionCaps, KeyType, Message, MessageBuilder,
    SecretKeyParamsBuilder, SignedPublicKey, SignedSecretKey, SubkeyParamsBuilder,
};
use pgp::crypto::ecc_curve::ECCCurve;
use pgp::crypto::hash::HashAlgorithm;
use pgp::crypto::sym::SymmetricKeyAlgorithm;
use pgp::ser::Serialize;
use pgp::types::{KeyDetails, Password};
use rand::thread_rng;
use std::path::PathBuf;

fn keys_dir(kind: &str) -> Result<PathBuf> {
    let dir = crate::paths::data_dir()
        .context("data directory not found")?
        .join("keys")
        .join(kind);
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    Ok(dir)
}

/// File name for an address: lowercase, only safe characters.
fn file_for(address: &str) -> String {
    let clean: String = address
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "@._-+".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{clean}.asc")
}

fn write_private(path: &std::path::Path, data: &[u8]) -> Result<()> {
    std::fs::write(path, data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Email addresses in a key's user IDs ("Name <a@b.it>" → "a@b.it").
pub fn addresses(key: &SignedPublicKey) -> Vec<String> {
    key.details
        .users
        .iter()
        .filter_map(|u| u.id.as_str())
        .map(|id| match (id.rfind('<'), id.rfind('>')) {
            (Some(a), Some(b)) if a < b => id[a + 1..b].to_string(),
            _ => id.to_string(),
        })
        .map(|a| a.trim().to_lowercase())
        .filter(|a| a.contains('@'))
        .collect()
}

pub fn fingerprint(key: &SignedPublicKey) -> String {
    let hex = key.fingerprint().to_string().to_uppercase();
    // Groups of 4 for reading.
    hex.as_bytes()
        .chunks(4)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------- own keys

/// Generates an Ed25519/Curve25519 key (widely supported, also by GnuPG 2.2).
pub fn generate(name: &str, address: &str, passphrase: &str) -> Result<SignedSecretKey> {
    let uid = if name.trim().is_empty() {
        format!("<{address}>")
    } else {
        format!("{name} <{address}>")
    };
    let mut sign = SubkeyParamsBuilder::default();
    sign.key_type(KeyType::Ed25519Legacy)
        .can_sign(true)
        .can_encrypt(EncryptionCaps::None)
        .can_authenticate(false);
    let mut enc = SubkeyParamsBuilder::default();
    enc.key_type(KeyType::ECDH(ECCCurve::Curve25519Legacy))
        .can_sign(false)
        .can_encrypt(EncryptionCaps::All)
        .can_authenticate(false);
    if !passphrase.is_empty() {
        sign.passphrase(Some(passphrase.to_string()));
        enc.passphrase(Some(passphrase.to_string()));
    }
    let mut params = SecretKeyParamsBuilder::default();
    params
        .key_type(KeyType::Ed25519Legacy)
        .can_certify(true)
        .can_sign(false)
        .can_encrypt(EncryptionCaps::None)
        .primary_user_id(uid)
        .subkeys(vec![sign.build()?, enc.build()?]);
    if !passphrase.is_empty() {
        params.passphrase(Some(passphrase.to_string()));
    }
    let key = params
        .build()?
        .generate(thread_rng())
        .map_err(|e| anyhow!("key generation: {e}"))?;
    Ok(key)
}

pub fn own_key(address: &str) -> Option<SignedSecretKey> {
    let path = keys_dir("own").ok()?.join(file_for(address));
    let data = std::fs::read(path).ok()?;
    SignedSecretKey::from_reader_single(&data[..])
        .ok()
        .map(|(k, _)| k)
}

pub fn save_own_key(address: &str, key: &SignedSecretKey) -> Result<()> {
    let armored = key.to_armored_string(ArmorOptions::default())?;
    write_private(&keys_dir("own")?.join(file_for(address)), armored.as_bytes())
}

/// Imports a secret key file (armored or binary) for `address`.
pub fn import_own_key(address: &str, path: &std::path::Path) -> Result<SignedSecretKey> {
    let data = std::fs::read(path)?;
    let (key, _) = SignedSecretKey::from_reader_single(&data[..]).context("not an OpenPGP secret key")?;
    key.verify_bindings().context("key self-signatures invalid")?;
    save_own_key(address, &key)?;
    Ok(key)
}

/// Armored public part of the account's key (for sharing).
pub fn own_public_armored(address: &str) -> Option<String> {
    own_key(address)?
        .to_public_key()
        .to_armored_string(ArmorOptions::default())
        .ok()
}

// ---------------------------------------------------------------- contacts

pub fn contact_key(address: &str) -> Option<SignedPublicKey> {
    let path = keys_dir("contacts").ok()?.join(file_for(address));
    let data = std::fs::read(path).ok()?;
    SignedPublicKey::from_reader_single(&data[..])
        .ok()
        .map(|(k, _)| k)
}

fn save_contact_key(address: &str, key: &SignedPublicKey) -> Result<()> {
    let armored = key.to_armored_string(ArmorOptions::default())?;
    write_private(&keys_dir("contacts")?.join(file_for(address)), armored.as_bytes())
}

/// A key learned from Autocrypt headers (see the module comment).
pub fn autocrypt_key(address: &str) -> Option<SignedPublicKey> {
    let path = keys_dir("autocrypt").ok()?.join(file_for(address));
    let data = std::fs::read(path).ok()?;
    SignedPublicKey::from_reader_single(&data[..])
        .ok()
        .map(|(k, _)| k)
}

/// The key to encrypt to: imported by hand, else learned from Autocrypt.
pub fn encryption_key(address: &str) -> Option<SignedPublicKey> {
    contact_key(address).or_else(|| autocrypt_key(address))
}

/// Imports public keys from a file; returns the addresses they cover.
pub fn import_contact_keys(path: &std::path::Path) -> Result<Vec<String>> {
    let data = std::fs::read(path)?;
    let (keys, _) = SignedPublicKey::from_reader_many(&data[..]).context("not an OpenPGP public key")?;
    let mut imported = Vec::new();
    for key in keys.flatten() {
        if key.verify_bindings().is_err() {
            continue;
        }
        for address in addresses(&key) {
            save_contact_key(&address, &key)?;
            imported.push(address);
        }
    }
    if imported.is_empty() {
        bail!("no usable public key with an email address in this file");
    }
    Ok(imported)
}

/// Correspondents we have a key for (imported or from Autocrypt).
pub fn contact_count() -> usize {
    let names = |kind: &str| -> Vec<std::ffi::OsString> {
        keys_dir(kind)
            .ok()
            .and_then(|d| std::fs::read_dir(d).ok())
            .map(|it| {
                it.flatten()
                    .filter(|e| e.path().extension().is_some_and(|x| x == "asc"))
                    .map(|e| e.file_name())
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut all = names("contacts");
    all.extend(names("autocrypt"));
    all.sort();
    all.dedup();
    all.len()
}

// ---------------------------------------------------------------- Autocrypt

/// `Autocrypt:` header value advertising our key (Autocrypt 1.1).
pub fn autocrypt_header(address: &str) -> Option<String> {
    let key = own_key(address)?.to_public_key();
    let mut bin = Vec::new();
    key.to_writer(&mut bin).ok()?;
    use base64::Engine;
    let data = base64::engine::general_purpose::STANDARD.encode(bin);
    Some(format!("addr={address}; keydata={data}"))
}

/// The key in an incoming `Autocrypt:` header, if it is well formed and
/// names the sender. Nothing proves the sender sent it: see the module
/// comment.
pub fn autocrypt_offer(from: &str, header: &str) -> Option<SignedPublicKey> {
    let mut addr = None;
    let mut keydata = None;
    for attr in header.split(';') {
        let (k, v) = attr.split_once('=').unwrap_or((attr, ""));
        match k.trim() {
            "addr" => addr = Some(v.trim().to_lowercase()),
            "keydata" => keydata = Some(v.split_whitespace().collect::<String>()),
            _ => {}
        }
    }
    let (addr, keydata) = (addr?, keydata?);
    if addr != from.trim().to_lowercase() {
        return None;
    }
    use base64::Engine;
    let bin = base64::engine::general_purpose::STANDARD.decode(keydata).ok()?;
    let (key, _) = SignedPublicKey::from_reader_single(&bin[..]).ok()?;
    (key.verify_bindings().is_ok() && addresses(&key).contains(&addr)).then_some(key)
}

/// Whether an offered Autocrypt key may be stored: only for senders we
/// hold no key for. A different key for a known sender is never taken
/// (it could come from anyone writing with that From address); the
/// message view reports it when such a key signed the message.
fn should_learn(imported: Option<&SignedPublicKey>, learned: Option<&SignedPublicKey>) -> bool {
    imported.is_none() && learned.is_none()
}

/// Stores the key offered in an Autocrypt header (see `autocrypt_offer`)
/// if we hold none for the sender. Returns true when it was saved.
pub fn learn_autocrypt(from: &str, key: &SignedPublicKey) -> bool {
    let addr = from.trim().to_lowercase();
    if !should_learn(contact_key(&addr).as_ref(), autocrypt_key(&addr).as_ref()) {
        return false;
    }
    let Ok(armored) = key.to_armored_string(ArmorOptions::default()) else {
        return false;
    };
    keys_dir("autocrypt")
        .and_then(|d| write_private(&d.join(file_for(&addr)), armored.as_bytes()))
        .is_ok()
}

// ---------------------------------------------------------------- operations

fn can_encrypt(sigs: &[pgp::packet::Signature]) -> bool {
    sigs.iter().any(|s| {
        let f = s.key_flags();
        f.encrypt_comms() || f.encrypt_storage()
    })
}

fn can_sign(sigs: &[pgp::packet::Signature]) -> bool {
    sigs.iter().any(|s| s.key_flags().sign())
}

/// Encrypts `data` to the given keys (each key's encryption subkeys);
/// returns an armored OpenPGP message.
pub fn encrypt(data: &[u8], recipients: &[SignedPublicKey]) -> Result<String> {
    let mut builder =
        MessageBuilder::from_bytes("", data.to_vec()).seipd_v1(thread_rng(), SymmetricKeyAlgorithm::AES256);
    let mut any = false;
    for key in recipients {
        let mut found = false;
        for sub in key.public_subkeys.iter().filter(|s| can_encrypt(&s.signatures)) {
            builder
                .encrypt_to_key(thread_rng(), sub)
                .map_err(|e| anyhow!("{e}"))?;
            found = true;
        }
        if !found {
            bail!("key {} has no encryption subkey", fingerprint(key));
        }
        any = true;
    }
    if !any {
        bail!("no recipients");
    }
    builder
        .to_armored_string(thread_rng(), ArmorOptions::default())
        .map_err(|e| anyhow!("{e}"))
}

/// Detached signature over `data` (armored), with the signing subkey if
/// there is one, otherwise the primary key.
pub fn sign(data: &[u8], key: &SignedSecretKey, passphrase: &str) -> Result<String> {
    let pw = Password::from(passphrase);
    let sig = match key.secret_subkeys.iter().find(|s| can_sign(&s.signatures)) {
        Some(sub) => {
            DetachedSignature::sign_binary_data(thread_rng(), &sub.key, &pw, HashAlgorithm::Sha256, data)
        }
        None => DetachedSignature::sign_binary_data(
            thread_rng(),
            &key.primary_key,
            &pw,
            HashAlgorithm::Sha256,
            data,
        ),
    }
    .map_err(|e| anyhow!("signing failed (wrong passphrase?): {e}"))?;
    Ok(sig.to_armored_string(ArmorOptions::default())?)
}

/// Checks a detached signature against a public key (primary or any subkey).
pub fn verify(data: &[u8], signature: &[u8], key: &SignedPublicKey) -> bool {
    let sig = match DetachedSignature::from_reader_single(signature) {
        Ok((s, _)) => s,
        Err(_) => return false,
    };
    sig.verify(&key.primary_key, data).is_ok()
        || key
            .public_subkeys
            .iter()
            .any(|s| sig.verify(&s.key, data).is_ok())
}

/// Decrypts an OpenPGP message (armored or binary).
pub fn decrypt(data: &[u8], key: &SignedSecretKey, passphrase: &str) -> Result<Vec<u8>> {
    let text = String::from_utf8_lossy(data);
    let msg = if text.contains("-----BEGIN PGP MESSAGE-----") {
        let start = text.find("-----BEGIN PGP MESSAGE-----").unwrap_or(0);
        Message::from_string(&text[start..]).map(|(m, _)| m)
    } else {
        Message::from_bytes(data)
    }
    .map_err(|e| anyhow!("not an OpenPGP message: {e}"))?;
    let mut plain = msg
        .decrypt(&Password::from(passphrase), key)
        .map_err(|e| anyhow!("{e}"))?;
    if plain.is_compressed() {
        plain = plain.decompress().map_err(|e| anyhow!("{e}"))?;
    }
    plain.as_data_vec().map_err(|e| anyhow!("{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_verify_encrypt_decrypt_roundtrip() {
        let alice = generate("Alice", "alice@example.com", "pw").unwrap();
        let alice_pub = alice.to_public_key();
        assert_eq!(addresses(&alice_pub), vec!["alice@example.com"]);

        let data = b"Content-Type: text/plain\r\n\r\nhello\r\n";
        let sig = sign(data, &alice, "pw").unwrap();
        assert!(verify(data, sig.as_bytes(), &alice_pub));
        assert!(!verify(b"tampered", sig.as_bytes(), &alice_pub));
        assert!(sign(data, &alice, "wrong").is_err());

        let armored = encrypt(data, std::slice::from_ref(&alice_pub)).unwrap();
        assert!(armored.starts_with("-----BEGIN PGP MESSAGE-----"));
        assert_eq!(decrypt(armored.as_bytes(), &alice, "pw").unwrap(), data);
    }

    #[test]
    fn autocrypt_offers_never_replace_keys() {
        use base64::Engine;
        let alice = generate("Alice", "alice@example.com", "")
            .unwrap()
            .to_public_key();
        let mut bin = Vec::new();
        alice.to_writer(&mut bin).unwrap();
        let header = format!(
            "addr=alice@example.com; keydata={}",
            base64::engine::general_purpose::STANDARD.encode(bin)
        );
        let offered = autocrypt_offer("Alice@Example.com", &header).unwrap();
        assert_eq!(offered.fingerprint(), alice.fingerprint());
        // Only for the address in From.
        assert!(autocrypt_offer("mallory@example.com", &header).is_none());
        let other = generate("Mallory", "alice@example.com", "")
            .unwrap()
            .to_public_key();
        assert!(should_learn(None, None));
        assert!(!should_learn(Some(&other), None));
        assert!(!should_learn(None, Some(&other)));
    }

    #[test]
    fn file_names_are_safe() {
        assert_eq!(file_for(" A.B@Example.com "), "a.b@example.com.asc");
        assert_eq!(file_for("../../etc/passwd"), ".._.._etc_passwd.asc");
    }

    /// Writes artifacts for a manual GnuPG interop check (see README).
    #[test]
    #[ignore = "writes files for gpg interop"]
    fn gpg_interop_artifacts() {
        let dir = std::path::PathBuf::from(std::env::var("FERROWL_GPG_DIR").expect("FERROWL_GPG_DIR"));
        let key = generate("Ferrowl Test", "test@example.com", "").unwrap();
        let public = key.to_public_key();
        std::fs::write(
            dir.join("pub.asc"),
            public.to_armored_string(ArmorOptions::default()).unwrap(),
        )
        .unwrap();
        std::fs::write(
            dir.join("sec.asc"),
            key.to_armored_string(ArmorOptions::default()).unwrap(),
        )
        .unwrap();
        let data = b"Content-Type: text/plain\r\n\r\nhello gpg\r\n";
        std::fs::write(dir.join("data.txt"), data).unwrap();
        std::fs::write(dir.join("data.sig"), sign(data, &key, "").unwrap()).unwrap();
        std::fs::write(dir.join("msg.asc"), encrypt(data, &[public]).unwrap()).unwrap();
    }

    /// Cross-check with files produced by GnuPG (see `gpg_interop_artifacts`).
    #[test]
    #[ignore = "needs gpg-produced files"]
    fn gpg_interop_check() {
        let dir = std::path::PathBuf::from(std::env::var("FERROWL_GPG_DIR").expect("FERROWL_GPG_DIR"));
        // GnuPG → Ferrowl: decrypt a message gpg encrypted to our key.
        let (sec, _) =
            SignedSecretKey::from_reader_single(&std::fs::read(dir.join("sec.asc")).unwrap()[..]).unwrap();
        let plain = decrypt(&std::fs::read(dir.join("fromgpg.asc")).unwrap(), &sec, "").unwrap();
        assert_eq!(plain, b"hello from gpg\n");
        // Ferrowl → GnuPG: encrypt and sign for a gpg-generated key.
        let (gpg_pub, _) =
            SignedPublicKey::from_reader_single(&std::fs::read(dir.join("gpgpub.asc")).unwrap()[..]).unwrap();
        let to_gpg = encrypt(b"hello to gpg\n", &[gpg_pub]).unwrap();
        std::fs::write(dir.join("togpg.asc"), &to_gpg).unwrap();
        // A secret key exported from GnuPG works in Ferrowl (import path).
        let (gpg_sec, _) =
            SignedSecretKey::from_reader_single(&std::fs::read(dir.join("gpgsec.asc")).unwrap()[..]).unwrap();
        gpg_sec.verify_bindings().unwrap();
        assert_eq!(
            decrypt(to_gpg.as_bytes(), &gpg_sec, "").unwrap(),
            b"hello to gpg\n"
        );
        assert!(sign(b"x", &gpg_sec, "").is_ok());
    }
}
