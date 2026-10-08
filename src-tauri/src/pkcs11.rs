//! Linux: signing with a DSC token or smart card through its PKCS#11 driver. The drivers looked in are
//! p11-kit's proxy (every driver registered with the system, OpenSC's included), OpenSC itself, the paths the
//! usual DSC tokens' Linux drivers install to, and one chosen in Settings. The private key never leaves the
//! token: it signs the signature's attributes, and the CMS around that signature is put together here (the
//! same layout as on Windows and for certificate files: SHA-256, the chain, signing-certificate-v2).
//!
//! The token's PIN is typed into Paginaut (Windows' drivers ask in their own window) and used for the one
//! signature: it's handed to the token and dropped.

use cryptoki::context::{CInitializeArgs, CInitializeFlags, Pkcs11};
use cryptoki::error::RvError;
use cryptoki::mechanism::Mechanism;
use cryptoki::object::{Attribute, AttributeType, KeyType, ObjectClass};
use cryptoki::session::UserType;
use cryptoki::types::AuthPin;
use openssl::hash::{hash, MessageDigest};
use openssl::x509::X509;
use parking_lot::Mutex;
use std::path::{Path, PathBuf};

/// The DSC tokens' drivers where their Linux packages put them (ePass2003, ProxKey, mToken, HyperPKI, SafeNet
/// eToken, Gemalto IDPrime, Bit4id), and OpenSC for smart cards.
const KNOWN: &[&str] = &[
    "/usr/lib/x86_64-linux-gnu/p11-kit-proxy.so",
    "/usr/lib/aarch64-linux-gnu/p11-kit-proxy.so",
    "/usr/lib/x86_64-linux-gnu/opensc-pkcs11.so",
    "/usr/lib/aarch64-linux-gnu/opensc-pkcs11.so",
    "/usr/lib/libcastle.so.1.0.0",
    "/usr/lib/x86_64-linux-gnu/libcastle.so.1.0.0",
    "/usr/lib/WatchData/ProxKey/lib/libwdpkcs_SignatureP11.so",
    "/usr/lib/WatchData/HyperPKI/lib/libwdpkcs_HyperPKI.so",
    "/opt/LongmaiMtokenCryptoID/lib/libmtoken_csp11.so",
    "/usr/lib/libmtoken_csp11.so",
    "/usr/lib/libeTPkcs11.so",
    "/usr/lib/libIDPrimePKCS11.so",
    "/usr/lib/bit4id/libbit4ipki.so",
];

/// A driver chosen in Settings (Signature › Token driver).
static CHOSEN: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn set_module(path: Option<String>) {
    *CHOSEN.lock() = path.filter(|p| !p.is_empty()).map(PathBuf::from);
}

/// The drivers to look in, each once.
fn modules() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for p in CHOSEN.lock().iter().cloned().chain(KNOWN.iter().map(PathBuf::from)) {
        let real = std::fs::canonicalize(&p).unwrap_or(p);
        if real.is_file() && !out.contains(&real) {
            out.push(real);
        }
    }
    out
}

/// A certificate on a token, with what finds it again.
pub struct TokenCert {
    pub cert: X509,
    pub module: PathBuf,
    pub slot: u64,
    pub id: Vec<u8>,
    pub label: String,
    /// The token has its own keypad: no PIN is typed into Paginaut.
    pub pinpad: bool,
    /// The token's other certificates (the issuer's chain, when it carries it).
    pub others: Vec<X509>,
}

fn open(module: &Path) -> Result<Pkcs11, String> {
    let p = Pkcs11::new(module).map_err(|e| format!("Couldn't load the token driver {}: {e}", module.display()))?;
    p.initialize(CInitializeArgs::new(CInitializeFlags::OS_LOCKING_OK)).map_err(|e| format!("The token driver didn't start: {e}"))?;
    Ok(p)
}

/// Every certificate with a private key on the tokens plugged in.
pub fn certificates() -> Vec<TokenCert> {
    let mut out: Vec<TokenCert> = Vec::new();
    for module in modules() {
        let Ok(p11) = open(&module) else { continue };
        let Ok(slots) = p11.get_slots_with_token() else { continue };
        for slot in slots {
            let Ok(info) = p11.get_token_info(slot) else { continue };
            let Ok(session) = p11.open_ro_session(slot) else { continue };
            let Ok(handles) = session.find_objects(&[Attribute::Class(ObjectClass::CERTIFICATE)]) else { continue };
            let mut certs: Vec<(X509, Vec<u8>)> = Vec::new();
            for h in handles {
                let Ok(attrs) = session.get_attributes(h, &[AttributeType::Value, AttributeType::Id]) else { continue };
                let (mut der, mut id) = (Vec::new(), Vec::new());
                for a in attrs {
                    match a {
                        Attribute::Value(v) => der = v,
                        Attribute::Id(v) => id = v,
                        _ => {}
                    }
                }
                if let Ok(c) = X509::from_der(&der) {
                    certs.push((c, id));
                }
            }
            // The ones whose key is on the token too can sign: the key (or, before logging in, which hides private
            // objects, its public half) carries the certificate's id.
            let has_key = |id: &[u8]| {
                !id.is_empty()
                    && [ObjectClass::PRIVATE_KEY, ObjectClass::PUBLIC_KEY]
                        .iter()
                        .any(|&class| session.find_objects(&[Attribute::Class(class), Attribute::Id(id.to_vec())]).is_ok_and(|k| !k.is_empty()))
            };
            for (c, id) in &certs {
                // Some tokens list keys only after logging in: a certificate that issued none of the others and
                // isn't self-signed (not an authority's) is offered anyway.
                let name = c.subject_name().to_der().ok();
                let issues_others = certs.iter().any(|(o, _)| !std::ptr::eq(o, c) && o.issuer_name().to_der().ok() == name);
                let self_signed = c.issuer_name().to_der().ok() == name;
                if !(has_key(id) || (!issues_others && !self_signed)) {
                    continue;
                }
                let thumb = c.digest(MessageDigest::sha1()).map(|d| d.to_vec()).unwrap_or_default();
                if out.iter().any(|t| t.cert.digest(MessageDigest::sha1()).map(|d| d.to_vec()).unwrap_or_default() == thumb) {
                    continue;
                }
                out.push(TokenCert {
                    cert: c.clone(),
                    module: module.clone(),
                    slot: slot.id(),
                    id: id.clone(),
                    label: info.label().trim().to_string(),
                    pinpad: info.protected_authentication_path(),
                    others: certs.iter().filter(|(o, _)| !std::ptr::eq(o, c)).map(|(o, _)| o.clone()).collect(),
                });
            }
        }
    }
    out
}

/// Signs `data` (the DER of the signed attributes, as a SET) with the token's key for `t`. RSA keys sign with
/// SHA-256 in the token, or with the digest made here when the token can't hash (older drivers); EC keys
/// return r‖s, turned into DER here.
pub fn sign(t: &TokenCert, pin: Option<&str>, data: &[u8]) -> Result<(Vec<u8>, bool), String> {
    let p11 = open(&t.module)?;
    let slot = p11.get_slots_with_token().map_err(|e| e.to_string())?.into_iter().find(|s| s.id() == t.slot).ok_or("The token was unplugged.")?;
    let session = p11.open_ro_session(slot).map_err(|e| format!("Couldn't open the token: {e}"))?;
    let pin = if t.pinpad { None } else { Some(AuthPin::new(pin.unwrap_or_default().into())) };
    session.login(UserType::User, pin.as_ref()).map_err(|e| match e {
        cryptoki::error::Error::Pkcs11(RvError::PinIncorrect, _) | cryptoki::error::Error::Pkcs11(RvError::PinLenRange, _) => {
            "Wrong PIN. Tokens lock after a few wrong tries: check it before trying again.".to_string()
        }
        cryptoki::error::Error::Pkcs11(RvError::PinLocked, _) => "The token's PIN is locked. Unlock it with the token's own tool.".to_string(),
        e => format!("The token didn't accept the PIN: {e}"),
    })?;
    let key = session
        .find_objects(&[Attribute::Class(ObjectClass::PRIVATE_KEY), Attribute::Id(t.id.clone())])
        .map_err(|e| e.to_string())?
        .into_iter()
        .next()
        .ok_or("The certificate's key isn't on the token.")?;
    let kind = session.get_attributes(key, &[AttributeType::KeyType]).map_err(|e| e.to_string())?.into_iter().find_map(|a| match a {
        Attribute::KeyType(k) => Some(k),
        _ => None,
    });
    let digest = hash(MessageDigest::sha256(), data).map_err(|e| e.to_string())?;
    let r = if kind == Some(KeyType::EC) {
        let rs = session.sign(&Mechanism::Ecdsa, key, &digest).map_err(|e| format!("The token couldn't sign: {e}"))?;
        let (r, s) = rs.split_at(rs.len() / 2);
        Ok((ecdsa_der(r, s), true))
    } else {
        match session.sign(&Mechanism::Sha256RsaPkcs, key, data) {
            Ok(sig) => Ok((sig, false)),
            Err(_) => {
                // DigestInfo { sha256, digest }, for tokens that sign only what they're given.
                let mut info = vec![0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05, 0x00, 0x04, 0x20];
                info.extend_from_slice(&digest);
                session.sign(&Mechanism::RsaPkcs, key, &info).map(|s| (s, false)).map_err(|e| format!("The token couldn't sign: {e}"))
            }
        }
    };
    let _ = session.logout();
    r
}

/// ECDSA's r and s as DER: SEQUENCE { INTEGER r, INTEGER s } (P-521's is longer than 127 bytes: a long-form length).
fn ecdsa_der(r: &[u8], s: &[u8]) -> Vec<u8> {
    let tlv = |tag: u8, body: Vec<u8>| {
        let mut out = vec![tag];
        if body.len() < 0x80 {
            out.push(body.len() as u8);
        } else {
            let len: Vec<u8> = body.len().to_be_bytes().into_iter().skip_while(|&b| b == 0).collect();
            out.push(0x80 | len.len() as u8);
            out.extend(len);
        }
        out.extend(body);
        out
    };
    let int = |v: &[u8]| {
        let v: Vec<u8> = v.iter().copied().skip_while(|&b| b == 0).collect();
        let mut body = if v.first().is_some_and(|&b| b & 0x80 != 0) { vec![0] } else { Vec::new() };
        body.extend(if v.is_empty() { vec![0] } else { v });
        tlv(0x02, body)
    };
    tlv(0x30, [int(r), int(s)].concat())
}

#[cfg(test)]
mod tests {
    #[test]
    fn ecdsa_lengths() {
        // P-256: short lengths; a high bit gets a leading zero.
        let d = super::ecdsa_der(&[0x80; 32], &[0x01; 32]);
        assert_eq!(&d[..4], &[0x30, 2 + 33 + 2 + 32, 0x02, 33]);
        // P-521 (66-byte r and s): the sequence needs a long-form length.
        let d = super::ecdsa_der(&[0xff; 66], &[0xff; 66]);
        assert_eq!(&d[..3], &[0x30, 0x81, 2 * (2 + 67)]);
        assert_eq!(d.len(), 3 + 2 * (2 + 67));
    }
}
