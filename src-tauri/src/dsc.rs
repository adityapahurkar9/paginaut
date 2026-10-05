//! Digital signatures with certificates from the Windows certificate store (on Linux, from .pfx/.p12
//! files through OpenSSL: `imp` below): the
//! store is where a DSC token's driver puts its certificates (Adobe finds them
//! there too). Signing goes through Windows' own crypto, so the token's driver
//! asks for its PIN in its own window; Paginaut never sees it.
//!
//! The signature is a CMS (PKCS #7) SignedData, detached, SHA-256, with the
//! certificate chain and the signing-certificate-v2 attribute (as PAdES wants).
//! Optionally it carries a trusted timestamp (RFC 3161) from a timestamp service,
//! so it can still be checked after the certificate expires. Certificates can also
//! come from a .pfx/.p12 file, held in memory for the one signature.
use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CertSummary {
    /// SHA-1 thumbprint, hex (how Windows identifies the certificate).
    pub thumb: String,
    pub name: String,
    pub issuer: String,
    pub email: String,
    pub org: String,
    /// Seconds since 1970.
    pub not_before: i64,
    pub not_after: i64,
    /// The key's provider (the token's driver, or Windows' software store).
    pub provider: String,
    /// The key is on a token or smart card.
    pub hardware: bool,
    /// Within its dates.
    pub current: bool,
    /// Looks meant for signing documents (issued by an authority, for signing
    /// or email). Others (e.g. self-signed ones apps make for themselves) are
    /// only shown on request.
    pub likely: bool,
}

/// Result of checking one signature's CMS blob against the bytes it covers.
#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct SigCheck {
    /// The covered bytes are exactly what was signed.
    pub intact: bool,
    pub signer: String,
    pub issuer: String,
    pub email: String,
    /// "trusted", "expired" (valid when signed, expired since), "untrusted" (root not trusted by Windows),
    /// "incomplete" (issuer certificate missing), "revoked", "invalid", or "unknown".
    pub trust: String,
    pub detail: String,
    pub not_after: i64,
    /// Trusted timestamp (seconds since 1970), 0 when the signature has none that matches it.
    pub timestamp: i64,
    /// Who issued the timestamp.
    pub tsa: String,
    /// Windows trusts the timestamp service.
    pub timestamp_trusted: bool,
}

/// An online check of whether the signer's certificate has been revoked.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Revocation {
    /// "good", "revoked" or "unknown" (offline, or the issuer publishes no revocation information).
    pub status: String,
    pub detail: String,
}

#[cfg(windows)]
pub use imp::*;

#[cfg(windows)]
mod imp {
    use super::{CertSummary, Revocation, SigCheck};
    use std::ffi::c_void;
    use windows::core::{s, w, PCSTR, PSTR};
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::Security::Cryptography::*;

    const ENC: CERT_QUERY_ENCODING_TYPE = CERT_QUERY_ENCODING_TYPE(X509_ASN_ENCODING.0 | PKCS_7_ASN_ENCODING.0);
    const OID_SHA256: &[u8] = b"2.16.840.1.101.3.4.2.1\0";
    const OID_SHA1: &[u8] = b"1.3.14.3.2.26\0";
    const OID_SIGNING_TIME: &[u8] = b"1.2.840.113549.1.9.5\0";
    const OID_SIGNING_CERT_V2: &[u8] = b"1.2.840.113549.1.9.16.2.47\0";
    const OID_TIMESTAMP_TOKEN: &[u8] = b"1.2.840.113549.1.9.16.2.14\0";

    fn pstr(b: &'static [u8]) -> PSTR {
        PSTR(b.as_ptr() as *mut u8)
    }

    fn filetime_to_unix(f: FILETIME) -> i64 {
        let t = ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64;
        (t / 10_000_000) as i64 - 11_644_473_600
    }

    fn unix_to_filetime(s: i64) -> FILETIME {
        let t = ((s + 11_644_473_600) as u64) * 10_000_000;
        FILETIME { dwLowDateTime: t as u32, dwHighDateTime: (t >> 32) as u32 }
    }

    fn now_unix() -> i64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
    }

    /// A friendly message for the errors people actually meet with tokens.
    fn explain(e: &windows::core::Error) -> String {
        match e.code().0 as u32 {
            0x8010_006E | 0x8007_04C7 => "Signing was cancelled.".into(),
            0x8010_006B => "The PIN was wrong. The token allows only a few tries before it locks.".into(),
            0x8010_006C => "The token's PIN is locked after too many wrong tries. Unlock it with the token's own software.".into(),
            0x8010_000C | 0x8010_0069 | 0x8010_0017 | 0x8009_0016 | 0x8009_200B | 0x8010_002E => {
                "The token isn't plugged in, or its driver can't reach the key. Plug it in and try again.".into()
            }
            _ => format!("Windows couldn't sign with this certificate: {} (0x{:08X})", e.message(), e.code().0 as u32),
        }
    }

    // ------------------------------------------------------------ certificates

    /// A certificate from the store (owned reference). `key` is set only for
    /// test certificates, whose key lives in memory: Windows is never asked to
    /// find it (it would go looking for a smart card). `store` holds a .pfx
    /// file's certificates (and its key) in memory while the certificate is used.
    pub struct Cert(*const CERT_CONTEXT, Option<NCRYPT_KEY_HANDLE>, Option<HCERTSTORE>);

    impl Drop for Cert {
        fn drop(&mut self) {
            unsafe {
                let _ = CertFreeCertificateContext(Some(self.0));
                if let Some(k) = self.1 {
                    let _ = NCryptFreeObject(k.into());
                }
                if let Some(st) = self.2 {
                    let _ = CertCloseStore(Some(st), 0);
                }
            }
        }
    }

    fn open_my() -> Result<HCERTSTORE, String> {
        unsafe {
            CertOpenStore(
                CERT_STORE_PROV_SYSTEM_W,
                CERT_QUERY_ENCODING_TYPE(0),
                None,
                CERT_OPEN_STORE_FLAGS(CERT_SYSTEM_STORE_CURRENT_USER | CERT_STORE_READONLY_FLAG.0 | CERT_STORE_OPEN_EXISTING_FLAG.0),
                Some(w!("MY").as_ptr() as *const c_void),
            )
        }
        .map_err(|e| format!("Couldn't open your certificate store: {}", e.message()))
    }

    fn name_attr(c: *const CERT_CONTEXT, oid: PCSTR, issuer: bool) -> String {
        let flags = if issuer { CERT_NAME_ISSUER_FLAG } else { 0 };
        let mut buf = [0u16; 512];
        let n = unsafe { CertGetNameStringW(c, CERT_NAME_ATTR_TYPE, flags, Some(oid.0 as *const c_void), Some(&mut buf)) };
        String::from_utf16_lossy(&buf[..(n as usize).saturating_sub(1)])
    }

    fn display_name(c: *const CERT_CONTEXT, issuer: bool) -> String {
        let flags = if issuer { CERT_NAME_ISSUER_FLAG } else { 0 };
        let mut buf = [0u16; 512];
        let n = unsafe { CertGetNameStringW(c, CERT_NAME_SIMPLE_DISPLAY_TYPE, flags, None, Some(&mut buf)) };
        String::from_utf16_lossy(&buf[..(n as usize).saturating_sub(1)])
    }

    fn prop(c: *const CERT_CONTEXT, id: u32) -> Option<Vec<u8>> {
        let mut len = 0u32;
        unsafe { CertGetCertificateContextProperty(c, id, None, &mut len) }.ok()?;
        let mut v = vec![0u8; len as usize];
        unsafe { CertGetCertificateContextProperty(c, id, Some(v.as_mut_ptr() as *mut c_void), &mut len) }.ok()?;
        v.truncate(len as usize);
        Some(v)
    }

    fn provider(c: *const CERT_CONTEXT) -> Option<String> {
        let v = prop(c, CERT_KEY_PROV_INFO_PROP_ID)?;
        let info = unsafe { &*(v.as_ptr() as *const CRYPT_KEY_PROV_INFO) };
        if info.pwszProvName.is_null() {
            return Some(String::new());
        }
        Some(unsafe { info.pwszProvName.to_string() }.unwrap_or_default())
    }

    /// Windows' own software providers; anything else is a token or smart card driver.
    fn is_hardware(provider: &str) -> bool {
        let p = provider.to_ascii_lowercase();
        if p.contains("smart card") {
            return true;
        }
        let software = ["microsoft enhanced", "microsoft strong", "microsoft base cryptographic", "microsoft rsa", "microsoft dh",
            "microsoft dss", "microsoft software key storage", "microsoft platform crypto"];
        !(p.is_empty() || software.iter().any(|s| p.starts_with(s)))
    }

    /// Can make signatures: no key usage extension, or one that allows signing.
    fn can_sign(c: *const CERT_CONTEXT) -> bool {
        let mut ku = [0u8; 2];
        match unsafe { CertGetIntendedKeyUsage(ENC, (*c).pCertInfo, &mut ku) } {
            Ok(()) => ku[0] as u32 & (CERT_DIGITAL_SIGNATURE_KEY_USAGE | CERT_NON_REPUDIATION_KEY_USAGE) != 0,
            Err(_) => true,
        }
    }

    /// Issued by an authority (not self-signed), and either for any use or for
    /// signing documents, email or logging in (what DSC certificates carry).
    fn for_documents(c: *const CERT_CONTEXT) -> bool {
        unsafe {
            let info = &*(*c).pCertInfo;
            if CertCompareCertificateName(ENC, &info.Subject, &info.Issuer).as_bool() {
                return false;
            }
            const EXTENSION_ONLY: u32 = 2;
            let mut len = 0u32;
            if CertGetEnhancedKeyUsage(c, EXTENSION_ONLY, None, &mut len).is_err() {
                return true; // no extended key usage: any use
            }
            let mut buf = vec![0u64; (len as usize).div_ceil(8)];
            if CertGetEnhancedKeyUsage(c, EXTENSION_ONLY, Some(buf.as_mut_ptr() as *mut CTL_USAGE), &mut len).is_err() {
                return true;
            }
            let u = &*(buf.as_ptr() as *const CTL_USAGE);
            if u.cUsageIdentifier == 0 {
                return true;
            }
            let ok = ["1.3.6.1.4.1.311.10.3.12", "1.3.6.1.5.5.7.3.36", "1.2.840.113583.1.1.5", "1.3.6.1.5.5.7.3.4", "1.3.6.1.5.5.7.3.2", "2.5.29.37.0"];
            (0..u.cUsageIdentifier as usize).any(|i| {
                let oid = (*u.rgpszUsageIdentifier.add(i)).to_string().unwrap_or_default();
                ok.contains(&oid.as_str())
            })
        }
    }

    fn summary(c: *const CERT_CONTEXT) -> CertSummary {
        let info = unsafe { &*(*c).pCertInfo };
        let thumb = prop(c, CERT_SHA1_HASH_PROP_ID).map(|h| h.iter().map(|b| format!("{b:02X}")).collect()).unwrap_or_default();
        let provider = provider(c).unwrap_or_default();
        let (nb, na) = (filetime_to_unix(info.NotBefore), filetime_to_unix(info.NotAfter));
        let now = now_unix();
        let cn = name_attr(c, s!("2.5.4.3"), false);
        CertSummary {
            thumb,
            name: if cn.is_empty() { display_name(c, false) } else { cn },
            issuer: display_name(c, true),
            email: name_attr(c, s!("1.2.840.113549.1.9.1"), false),
            org: name_attr(c, s!("2.5.4.10"), false),
            not_before: nb,
            not_after: na,
            hardware: is_hardware(&provider),
            provider,
            current: nb <= now && now <= na,
            likely: for_documents(c),
        }
    }

    /// Certificates you can sign with: they have a private key (here or on a
    /// token) and a key usage that allows signing. Expired ones are listed too,
    /// marked, so you can see why they can't be used.
    pub fn list() -> Result<Vec<CertSummary>, String> {
        let store = open_my()?;
        let mut out = Vec::new();
        let mut c: *mut CERT_CONTEXT = std::ptr::null_mut();
        loop {
            c = unsafe { CertEnumCertificatesInStore(store, Some(c)) };
            if c.is_null() {
                break;
            }
            if provider(c).is_some() && can_sign(c) {
                out.push(summary(c));
            }
        }
        unsafe {
            let _ = CertCloseStore(Some(store), 0);
        }
        // Current ones first, newest first.
        out.sort_by(|a, b| b.likely.cmp(&a.likely).then(b.current.cmp(&a.current)).then(b.not_after.cmp(&a.not_after)));
        Ok(out)
    }

    impl Cert {
        pub fn find(thumb: &str) -> Result<Cert, String> {
            let bytes: Vec<u8> = (0..thumb.len() / 2).filter_map(|i| u8::from_str_radix(&thumb[i * 2..i * 2 + 2], 16).ok()).collect();
            let store = open_my()?;
            let blob = CRYPT_INTEGER_BLOB { cbData: bytes.len() as u32, pbData: bytes.as_ptr() as *mut u8 };
            let c = unsafe { CertFindCertificateInStore(store, ENC, 0, CERT_FIND_SHA1_HASH, Some(&blob as *const _ as *const c_void), None) };
            unsafe {
                let _ = CertCloseStore(Some(store), 0);
            }
            if c.is_null() {
                return Err("That certificate is no longer in your certificate store. Is the token plugged in?".into());
            }
            Ok(Cert(c, None, None))
        }

        /// A certificate and its key from a .pfx/.p12 file, held in memory for this signature only: nothing is
        /// added to your certificate store, and the key goes when it's done.
        pub fn from_pfx(path: &str, password: &str) -> Result<Cert, String> {
            let data = std::fs::read(path).map_err(|e| format!("Couldn't read the certificate file: {e}"))?;
            let blob = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
            let pw = windows::core::HSTRING::from(password);
            let store = unsafe { PFXImportCertStore(&blob, &pw, PKCS12_NO_PERSIST_KEY | PKCS12_PREFER_CNG_KSP) }.map_err(|e| match e.code().0 as u32 {
                0x8007_0056 => "The password is wrong.".to_string(),
                _ => format!("That isn't a certificate file Windows can read: {}", e.message()),
            })?;
            // The one with a key (the file may hold its issuers' certificates too).
            let mut found: *const CERT_CONTEXT = std::ptr::null();
            let mut c: *mut CERT_CONTEXT = std::ptr::null_mut();
            loop {
                c = unsafe { CertEnumCertificatesInStore(store, (!c.is_null()).then_some(c as *const CERT_CONTEXT)) };
                if c.is_null() {
                    break;
                }
                let has_key = prop(c, CERT_KEY_PROV_INFO_PROP_ID).is_some() || prop(c, CERT_NCRYPT_KEY_HANDLE_PROP_ID).is_some() || prop(c, CERT_KEY_CONTEXT_PROP_ID).is_some();
                if has_key && can_sign(c) {
                    found = unsafe { CertDuplicateCertificateContext(Some(c)) };
                    unsafe {
                        let _ = CertFreeCertificateContext(Some(c));
                    }
                    break;
                }
            }
            if found.is_null() {
                unsafe {
                    let _ = CertCloseStore(Some(store), 0);
                }
                return Err("The file has no certificate with a key to sign with.".into());
            }
            Ok(Cert(found, None, Some(store)))
        }

        pub fn summary(&self) -> CertSummary {
            summary(self.0)
        }

        /// CN, O, OU, email, country (for the signature's appearance).
        pub fn dn(&self) -> [String; 5] {
            [
                name_attr(self.0, s!("2.5.4.3"), false),
                name_attr(self.0, s!("2.5.4.10"), false),
                name_attr(self.0, s!("2.5.4.11"), false),
                name_attr(self.0, s!("1.2.840.113549.1.9.1"), false),
                name_attr(self.0, s!("2.5.4.6"), false),
            ]
        }

        fn der(&self) -> &[u8] {
            unsafe { std::slice::from_raw_parts((*self.0).pbCertEncoded, (*self.0).cbCertEncoded as usize) }
        }

        /// The certificate and the ones that issued it, as Windows finds them.
        fn chain(&self) -> Vec<Vec<u8>> {
            let mut out = vec![self.der().to_vec()];
            let para = CERT_CHAIN_PARA { cbSize: std::mem::size_of::<CERT_CHAIN_PARA>() as u32, ..Default::default() };
            let mut chain: *mut CERT_CHAIN_CONTEXT = std::ptr::null_mut();
            if unsafe { CertGetCertificateChain(None, self.0, None, self.2, &para, 0, None, &mut chain) }.is_ok() && !chain.is_null() {
                unsafe {
                    let ch = &*chain;
                    if ch.cChain > 0 {
                        let simple = &**ch.rgpChain;
                        for i in 1..simple.cElement as usize {
                            let el = &**simple.rgpElement.add(i);
                            let c = &*el.pCertContext;
                            out.push(std::slice::from_raw_parts(c.pbCertEncoded, c.cbCertEncoded as usize).to_vec());
                        }
                    }
                    CertFreeCertificateChain(chain);
                }
            }
            out
        }

        /// Signs `data`: a detached CMS SignedData. The token's driver may show
        /// its PIN window (over `hwnd` when given).
        pub fn sign(&self, data: &[u8], hwnd: isize) -> Result<Vec<u8>, String> {
            match self.sign_with(data, hwnd, OID_SHA256) {
                // Some older token drivers can't do SHA-256; SHA-1 still verifies everywhere.
                Err((e, true)) => self.sign_with(data, hwnd, OID_SHA1).map_err(|(e2, _)| format!("{e} / {e2}")),
                r => r.map_err(|(e, _)| e),
            }
        }

        /// Err((message, the algorithm wasn't supported)).
        fn sign_with(&self, data: &[u8], hwnd: isize, hash_oid: &'static [u8]) -> Result<Vec<u8>, (String, bool)> {
            if let Some(k) = self.1 {
                return self.encode(data, HCRYPTPROV_OR_NCRYPT_KEY_HANDLE(k.0), CERT_NCRYPT_KEY_SPEC, hash_oid);
            }
            // A .pfx file's key lives in memory, attached to the certificate (Windows' usual lookup goes looking
            // for a stored key and fails). The certificate owns it: it isn't freed here.
            if self.2.is_some() {
                if let Some(v) = prop(self.0, CERT_NCRYPT_KEY_HANDLE_PROP_ID) {
                    let mut h = [0u8; 8];
                    h[..v.len().min(8)].copy_from_slice(&v[..v.len().min(8)]);
                    return self.encode(data, HCRYPTPROV_OR_NCRYPT_KEY_HANDLE(usize::from_le_bytes(h)), CERT_NCRYPT_KEY_SPEC, hash_oid);
                }
                if let Some(v) = prop(self.0, CERT_KEY_CONTEXT_PROP_ID) {
                    // SAFETY: the property is a CERT_KEY_CONTEXT, copied into an aligned value.
                    let kc: CERT_KEY_CONTEXT = unsafe { std::ptr::read_unaligned(v.as_ptr() as *const CERT_KEY_CONTEXT) };
                    let spec = CERT_KEY_SPEC(kc.dwKeySpec);
                    let key = if spec == CERT_NCRYPT_KEY_SPEC { unsafe { kc.Anonymous.hNCryptKey.0 } } else { unsafe { kc.Anonymous.hCryptProv } };
                    return self.encode(data, HCRYPTPROV_OR_NCRYPT_KEY_HANDLE(key), spec, hash_oid);
                }
            }
            let mut key = HCRYPTPROV_OR_NCRYPT_KEY_HANDLE(0);
            let mut spec = CERT_KEY_SPEC(0);
            let mut must_free = windows::core::BOOL(0);
            let mut flags = CRYPT_ACQUIRE_PREFER_NCRYPT_KEY_FLAG | CRYPT_ACQUIRE_COMPARE_KEY_FLAG;
            let hwnd_val = hwnd;
            let mut param: Option<*const c_void> = None;
            if hwnd != 0 {
                flags |= CRYPT_ACQUIRE_WINDOW_HANDLE_FLAG;
                param = Some(&hwnd_val as *const isize as *const c_void);
            }
            unsafe { CryptAcquireCertificatePrivateKey(self.0, flags, param, &mut key, Some(&mut spec), Some(&mut must_free)) }
                .map_err(|e| (explain(&e), false))?;
            let res = self.encode(data, key, spec, hash_oid);
            if must_free.as_bool() {
                unsafe {
                    if spec == CERT_NCRYPT_KEY_SPEC {
                        let _ = NCryptFreeObject(NCRYPT_HANDLE(key.0));
                    } else {
                        let _ = CryptReleaseContext(key.0, 0);
                    }
                }
            }
            res
        }

        fn encode(&self, data: &[u8], key: HCRYPTPROV_OR_NCRYPT_KEY_HANDLE, spec: CERT_KEY_SPEC, hash_oid: &'static [u8]) -> Result<Vec<u8>, (String, bool)> {
            // Signed attributes: signing time, and the certificate's hash (signing-certificate-v2).
            // Windows adds content type and message digest itself.
            let ft = unix_to_filetime(now_unix());
            let mut time = vec![0u8; 64];
            let mut tlen = time.len() as u32;
            unsafe { CryptEncodeObject(ENC, PKCS_UTC_TIME, &ft as *const _ as *const c_void, Some(time.as_mut_ptr()), &mut tlen) }
                .map_err(|e| (e.message(), false))?;
            time.truncate(tlen as usize);
            let mut hash = [0u8; 32];
            let mut hlen = 32u32;
            unsafe { CryptHashCertificate2(BCRYPT_SHA256_ALGORITHM, 0, None, Some(self.der()), Some(hash.as_mut_ptr()), &mut hlen) }
                .map_err(|e| (e.message(), false))?;
            // SigningCertificateV2 ::= SEQUENCE { SEQUENCE OF ESSCertIDv2 { certHash OCTET STRING } } (SHA-256 is the default).
            let mut scv2 = vec![0x30, 0x26, 0x30, 0x24, 0x30, 0x22, 0x04, 0x20];
            scv2.extend_from_slice(&hash);

            let mut time_blob = CRYPT_INTEGER_BLOB { cbData: time.len() as u32, pbData: time.as_mut_ptr() };
            let mut scv2_blob = CRYPT_INTEGER_BLOB { cbData: scv2.len() as u32, pbData: scv2.as_mut_ptr() };
            let mut attrs = [
                CRYPT_ATTRIBUTE { pszObjId: pstr(OID_SIGNING_TIME), cValue: 1, rgValue: &mut time_blob },
                CRYPT_ATTRIBUTE { pszObjId: pstr(OID_SIGNING_CERT_V2), cValue: 1, rgValue: &mut scv2_blob },
            ];
            let mut certs = self.chain();
            let mut cert_blobs: Vec<CRYPT_INTEGER_BLOB> =
                certs.iter_mut().map(|c| CRYPT_INTEGER_BLOB { cbData: c.len() as u32, pbData: c.as_mut_ptr() }).collect();

            let mut signer = CMSG_SIGNER_ENCODE_INFO {
                cbSize: std::mem::size_of::<CMSG_SIGNER_ENCODE_INFO>() as u32,
                pCertInfo: unsafe { (*self.0).pCertInfo },
                dwKeySpec: spec.0,
                HashAlgorithm: CRYPT_ALGORITHM_IDENTIFIER { pszObjId: pstr(hash_oid), Parameters: CRYPT_INTEGER_BLOB::default() },
                cAuthAttr: attrs.len() as u32,
                rgAuthAttr: attrs.as_mut_ptr(),
                ..Default::default()
            };
            if spec == CERT_NCRYPT_KEY_SPEC {
                signer.Anonymous.hNCryptKey = NCRYPT_KEY_HANDLE(key.0);
            } else {
                signer.Anonymous.hCryptProv = key.0;
            }
            let info = CMSG_SIGNED_ENCODE_INFO {
                cbSize: std::mem::size_of::<CMSG_SIGNED_ENCODE_INFO>() as u32,
                cSigners: 1,
                rgSigners: &mut signer,
                cCertEncoded: cert_blobs.len() as u32,
                rgCertEncoded: cert_blobs.as_mut_ptr(),
                ..Default::default()
            };
            let err = |e: windows::core::Error| {
                let bad_alg = e.code().0 as u32 == 0x8009_0008; // NTE_BAD_ALGID
                (explain(&e), bad_alg)
            };
            unsafe {
                let msg = CryptMsgOpenToEncode(ENC.0, CMSG_DETACHED_FLAG, CMSG_SIGNED, &info as *const _ as *const c_void, PCSTR::null(), None);
                if msg.is_null() {
                    return Err(err(windows::core::Error::from_thread()));
                }
                let res = (|| {
                    CryptMsgUpdate(msg, Some(data), true).map_err(err)?;
                    let mut len = 0u32;
                    CryptMsgGetParam(msg, CMSG_CONTENT_PARAM, 0, None, &mut len).map_err(err)?;
                    let mut out = vec![0u8; len as usize];
                    CryptMsgGetParam(msg, CMSG_CONTENT_PARAM, 0, Some(out.as_mut_ptr() as *mut c_void), &mut len).map_err(err)?;
                    out.truncate(len as usize);
                    Ok(out)
                })();
                let _ = CryptMsgClose(Some(msg));
                res
            }
        }
    }

    /// For tests only: a throwaway self-signed certificate whose key lives in
    /// memory (nothing is added to your certificate store).
    pub fn test_cert(name: &str) -> Result<Cert, String> {
        unsafe {
            let mut prov = NCRYPT_PROV_HANDLE(0);
            let step = |what: &'static str| move |e: windows::core::Error| format!("{what}: {}", e.message());
            NCryptOpenStorageProvider(&mut prov, MS_KEY_STORAGE_PROVIDER, 0).map_err(step("provider"))?;
            let mut key = NCRYPT_KEY_HANDLE(0);
            NCryptCreatePersistedKey(prov, &mut key, BCRYPT_RSA_ALGORITHM, None, CERT_KEY_SPEC(0), NCRYPT_FLAGS(0)).map_err(step("create key"))?;
            let bits = 2048u32.to_le_bytes();
            NCryptSetProperty(key.into(), NCRYPT_LENGTH_PROPERTY, &bits, NCRYPT_FLAGS(0)).map_err(step("key length"))?;
            NCryptFinalizeKey(key, NCRYPT_FLAGS(0)).map_err(step("finalize key"))?;
            let x500: Vec<u16> = format!("CN={name}, O=Paginaut test, C=IN").encode_utf16().chain(Some(0)).collect();
            let mut len = 0u32;
            CertStrToNameW(ENC, windows::core::PCWSTR(x500.as_ptr()), CERT_X500_NAME_STR, None, None, &mut len, None).map_err(|e| e.message())?;
            let mut subject = vec![0u8; len as usize];
            CertStrToNameW(ENC, windows::core::PCWSTR(x500.as_ptr()), CERT_X500_NAME_STR, None, Some(subject.as_mut_ptr()), &mut len, None)
                .map_err(|e| e.message())?;
            let _ = NCryptFreeObject(prov.into());
            // The certificate is put together and signed here, in memory. (Windows' own
            // self-signed helper looks for where the key is stored, and ends up asking
            // for a smart card.)
            let handle = HCRYPTPROV_OR_NCRYPT_KEY_HANDLE(key.0);
            let mut pki_len = 0u32;
            CryptExportPublicKeyInfo(handle, None, ENC, None, &mut pki_len).map_err(step("public key"))?;
            let mut pki_buf = vec![0u64; (pki_len as usize).div_ceil(8)];
            CryptExportPublicKeyInfo(handle, None, ENC, Some(pki_buf.as_mut_ptr() as *mut CERT_PUBLIC_KEY_INFO), &mut pki_len).map_err(step("public key"))?;
            let pki = &*(pki_buf.as_ptr() as *const CERT_PUBLIC_KEY_INFO);
            let mut serial = (now_unix() as u64).to_le_bytes();
            let name_blob = CRYPT_INTEGER_BLOB { cbData: len, pbData: subject.as_mut_ptr() };
            let alg = CRYPT_ALGORITHM_IDENTIFIER { pszObjId: pstr(b"1.2.840.113549.1.1.11\0"), Parameters: CRYPT_INTEGER_BLOB::default() };
            let info = CERT_INFO {
                dwVersion: 2, // v3
                SerialNumber: CRYPT_INTEGER_BLOB { cbData: serial.len() as u32, pbData: serial.as_mut_ptr() },
                SignatureAlgorithm: alg,
                Issuer: name_blob,
                NotBefore: unix_to_filetime(now_unix() - 3600),
                NotAfter: unix_to_filetime(now_unix() + 365 * 86400),
                Subject: name_blob,
                SubjectPublicKeyInfo: CERT_PUBLIC_KEY_INFO { Algorithm: pki.Algorithm, PublicKey: pki.PublicKey },
                ..Default::default()
            };
            let mut der_len = 0u32;
            CryptSignAndEncodeCertificate(Some(handle), None, ENC, X509_CERT_TO_BE_SIGNED, &info as *const _ as *const c_void, &alg, None, None, &mut der_len)
                .map_err(step("sign certificate"))?;
            let mut der = vec![0u8; der_len as usize];
            CryptSignAndEncodeCertificate(Some(handle), None, ENC, X509_CERT_TO_BE_SIGNED, &info as *const _ as *const c_void, &alg, None, Some(der.as_mut_ptr()), &mut der_len)
                .map_err(step("sign certificate"))?;
            der.truncate(der_len as usize);
            let c = CertCreateCertificateContext(ENC, &der);
            if c.is_null() {
                return Err(windows::core::Error::from_thread().message());
            }
            Ok(Cert(c, Some(key), None))
        }
    }

    // ------------------------------------------------------------ timestamps

    /// One parameter of a CMS message, in a buffer aligned for the structures some parameters are.
    unsafe fn msg_param(msg: *const c_void, param: u32) -> Result<Vec<u64>, String> {
        let mut len = 0u32;
        CryptMsgGetParam(msg, param, 0, None, &mut len).map_err(|e| e.message())?;
        let mut out = vec![0u64; (len as usize).div_ceil(8)];
        CryptMsgGetParam(msg, param, 0, Some(out.as_mut_ptr() as *mut c_void), &mut len).map_err(|e| e.message())?;
        Ok(out)
    }

    /// The same, as bytes.
    unsafe fn msg_bytes(msg: *const c_void, param: u32) -> Result<Vec<u8>, String> {
        let mut len = 0u32;
        CryptMsgGetParam(msg, param, 0, None, &mut len).map_err(|e| e.message())?;
        let mut out = vec![0u8; len as usize];
        CryptMsgGetParam(msg, param, 0, Some(out.as_mut_ptr() as *mut c_void), &mut len).map_err(|e| e.message())?;
        out.truncate(len as usize);
        Ok(out)
    }

    /// Opens a signature's CMS for reading (the caller closes it).
    unsafe fn open_cms(cms: &[u8]) -> Result<*mut c_void, String> {
        let msg = CryptMsgOpenToDecode(ENC.0, CMSG_DETACHED_FLAG, 0, None, None, None);
        if msg.is_null() {
            return Err(windows::core::Error::from_thread().message());
        }
        if let Err(e) = CryptMsgUpdate(msg, Some(cms), true) {
            let _ = CryptMsgClose(Some(msg));
            return Err(e.message());
        }
        Ok(msg)
    }

    /// A timestamp's time. Windows packs the timestamp's details unaligned, so the field is read as bytes.
    unsafe fn ts_time(ctx: *const CRYPT_TIMESTAMP_CONTEXT) -> i64 {
        let info = (*ctx).pTimeStamp;
        filetime_to_unix(std::ptr::read_unaligned(std::ptr::addr_of!((*info).ftTime)))
    }

    /// Adds a trusted timestamp to a signature: the timestamp service at `url` (RFC 3161) signs the signature's
    /// value together with the time, so the signature can still be checked after the certificate expires.
    /// Returns the new CMS and the time.
    pub fn add_timestamp(cms: &[u8], url: &str) -> Result<(Vec<u8>, i64), String> {
        unsafe {
            let msg = open_cms(cms)?;
            let res = (|| -> Result<(Vec<u8>, i64), String> {
                let digest = msg_bytes(msg, CMSG_ENCRYPTED_DIGEST)?;
                let mut ctx: *mut CRYPT_TIMESTAMP_CONTEXT = std::ptr::null_mut();
                let mut tsa: *mut CERT_CONTEXT = std::ptr::null_mut();
                let mut tsa_store = HCERTSTORE::default();
                CryptRetrieveTimeStamp(
                    &windows::core::HSTRING::from(url),
                    TIMESTAMP_VERIFY_CONTEXT_SIGNATURE,
                    30_000,
                    PCSTR(OID_SHA256.as_ptr()),
                    None,
                    &digest,
                    &mut ctx,
                    &mut tsa,
                    Some(&mut tsa_store),
                )
                .map_err(|e| format!("The timestamp service ({url}) didn't answer: {}", e.message()))?;
                let token = std::slice::from_raw_parts((*ctx).pbEncoded, (*ctx).cbEncoded as usize).to_vec();
                let time = ts_time(ctx);
                CryptMemFree(Some(ctx as *const c_void));
                if !tsa.is_null() {
                    let _ = CertFreeCertificateContext(Some(tsa));
                }
                if !tsa_store.is_invalid() {
                    let _ = CertCloseStore(Some(tsa_store), 0);
                }
                let mut value = CRYPT_INTEGER_BLOB { cbData: token.len() as u32, pbData: token.as_ptr() as *mut u8 };
                let attr = CRYPT_ATTRIBUTE { pszObjId: pstr(OID_TIMESTAMP_TOKEN), cValue: 1, rgValue: &mut value };
                let mut len = 0u32;
                CryptEncodeObject(ENC, PKCS_ATTRIBUTE, &attr as *const _ as *const c_void, None, &mut len).map_err(|e| e.message())?;
                let mut enc = vec![0u8; len as usize];
                CryptEncodeObject(ENC, PKCS_ATTRIBUTE, &attr as *const _ as *const c_void, Some(enc.as_mut_ptr()), &mut len).map_err(|e| e.message())?;
                enc.truncate(len as usize);
                let para = CMSG_CTRL_ADD_SIGNER_UNAUTH_ATTR_PARA {
                    cbSize: std::mem::size_of::<CMSG_CTRL_ADD_SIGNER_UNAUTH_ATTR_PARA>() as u32,
                    dwSignerIndex: 0,
                    blob: CRYPT_INTEGER_BLOB { cbData: enc.len() as u32, pbData: enc.as_mut_ptr() },
                };
                CryptMsgControl(msg, 0, CMSG_CTRL_ADD_SIGNER_UNAUTH_ATTR, Some(&para as *const _ as *const c_void)).map_err(|e| e.message())?;
                Ok((msg_bytes(msg, CMSG_ENCODED_MESSAGE)?, time))
            })();
            let _ = CryptMsgClose(Some(msg));
            res
        }
    }

    /// A signature's timestamp, if it has one that matches it: the time, who issued it, and whether Windows
    /// trusts that service.
    fn timestamp_of(cms: &[u8]) -> Option<(i64, String, bool)> {
        unsafe {
            let msg = open_cms(cms).ok()?;
            let res = (|| {
                let attrs = msg_param(msg, CMSG_SIGNER_UNAUTH_ATTR_PARAM).ok()?;
                let list = &*(attrs.as_ptr() as *const CRYPT_ATTRIBUTES);
                let oid = std::str::from_utf8(&OID_TIMESTAMP_TOKEN[..OID_TIMESTAMP_TOKEN.len() - 1]).ok()?;
                let token = (0..list.cAttr as usize).map(|i| &*list.rgAttr.add(i)).find(|a| a.pszObjId.to_string().ok().as_deref() == Some(oid) && a.cValue > 0)?;
                let v = &*token.rgValue;
                let token = std::slice::from_raw_parts(v.pbData, v.cbData as usize);
                let digest = msg_bytes(msg, CMSG_ENCRYPTED_DIGEST).ok()?;
                let mut ctx: *mut CRYPT_TIMESTAMP_CONTEXT = std::ptr::null_mut();
                let mut signer: *mut CERT_CONTEXT = std::ptr::null_mut();
                let mut store = HCERTSTORE::default();
                CryptVerifyTimeStampSignature(token, Some(&digest), None, &mut ctx, &mut signer, Some(&mut store)).ok()?;
                let time = ts_time(ctx);
                CryptMemFree(Some(ctx as *const c_void));
                let by = display_name(signer, false);
                let trusted = chain_status(signer, Some(store), Some(time)).0 == 0;
                let _ = CertFreeCertificateContext(Some(signer));
                let _ = CertCloseStore(Some(store), 0);
                Some((time, by, trusted))
            })();
            let _ = CryptMsgClose(Some(msg));
            res
        }
    }

    /// Asks the signer's certificate authority, online, whether the certificate has been revoked (OCSP or its
    /// revocation list, as the certificate names them; Windows caches the answers).
    pub fn revocation(cms: &[u8]) -> Revocation {
        let cms = &cms[..der_len(cms)];
        let unknown = |d: &str| Revocation { status: "unknown".into(), detail: d.into() };
        unsafe {
            let Ok(store) = CryptGetMessageCertificates(ENC.0, None, 0, cms) else { return unknown("The signature's certificate couldn't be read.") };
            let signer = signer_in(cms, store);
            if signer.is_null() {
                let _ = CertCloseStore(Some(store), 0);
                return unknown("The signature's certificate couldn't be read.");
            }
            let para = CERT_CHAIN_PARA { cbSize: std::mem::size_of::<CERT_CHAIN_PARA>() as u32, dwUrlRetrievalTimeout: 15_000, ..Default::default() };
            let mut chain: *mut CERT_CHAIN_CONTEXT = std::ptr::null_mut();
            let ok = CertGetCertificateChain(None, signer, None, Some(store), &para, CERT_CHAIN_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT, None, &mut chain);
            let out = if ok.is_err() || chain.is_null() {
                unknown("Windows couldn't build the certificate's chain.")
            } else {
                let st = (*chain).TrustStatus.dwErrorStatus;
                // The signer's own certificate: its status, and whether an answer about revocation came back for it.
                let (own, answered) = match (*chain).cChain {
                    0 => (st, false),
                    _ => {
                        let simple = &**(*chain).rgpChain;
                        let el = &**simple.rgpElement;
                        (el.TrustStatus.dwErrorStatus, !el.pRevocationInfo.is_null() && simple.cElement > 1)
                    }
                };
                CertFreeCertificateChain(chain);
                if (st | own) & CERT_TRUST_IS_REVOKED != 0 {
                    Revocation { status: "revoked".into(), detail: "The certificate authority says this certificate has been revoked.".into() }
                } else if st & CERT_TRUST_IS_UNTRUSTED_ROOT != 0 {
                    unknown("Windows doesn't trust the authority that issued this certificate, so there's no one to ask.")
                } else if own & (CERT_TRUST_REVOCATION_STATUS_UNKNOWN | CERT_TRUST_IS_OFFLINE_REVOCATION) != 0 || !answered {
                    unknown("The certificate authority couldn't be reached, or doesn't publish whether its certificates were revoked.")
                } else {
                    Revocation { status: "good".into(), detail: "The certificate authority says this certificate hasn't been revoked.".into() }
                }
            };
            let _ = CertFreeCertificateContext(Some(signer));
            let _ = CertCloseStore(Some(store), 0);
            out
        }
    }

    // ------------------------------------------------------------ checking

    /// Length of the DER object at the start of `b` (the CMS blob is padded with zeros).
    fn der_len(b: &[u8]) -> usize {
        if b.len() < 2 {
            return b.len();
        }
        let l = b[1] as usize;
        let total = if l < 0x80 {
            2 + l
        } else {
            let n = l & 0x7f;
            if n == 0 || n > 4 || b.len() < 2 + n {
                return b.len();
            }
            2 + n + b[2..2 + n].iter().fold(0usize, |a, &x| (a << 8) | x as usize)
        };
        total.min(b.len())
    }

    /// Checks a signature: that `data` is what was signed, who signed it, and
    /// whether Windows trusts the certificate (at signing time if it has
    /// expired since). Revocation isn't checked (no network).
    pub fn verify(data: &[u8], cms: &[u8], signed_at: Option<i64>) -> SigCheck {
        let cms = &cms[..der_len(cms)];
        let mut out = SigCheck { trust: "unknown".into(), ..Default::default() };
        let para = CRYPT_VERIFY_MESSAGE_PARA {
            cbSize: std::mem::size_of::<CRYPT_VERIFY_MESSAGE_PARA>() as u32,
            dwMsgAndCertEncodingType: ENC.0,
            ..Default::default()
        };
        let ptrs = [data.as_ptr()];
        let lens = [data.len() as u32];
        let mut signer: *mut CERT_CONTEXT = std::ptr::null_mut();
        let res = unsafe { CryptVerifyDetachedMessageSignature(&para, 0, cms, 1, ptrs.as_ptr(), lens.as_ptr(), Some(&mut signer)) };
        match res {
            Ok(()) => out.intact = true,
            Err(e) => {
                out.detail = match e.code().0 as u32 {
                    // Hash value / bad signature: the signed bytes differ from what was signed.
                    0x8009_1007 | 0x8009_0006 => "The document was altered after it was signed: its signed content no longer matches.".into(),
                    _ => format!("The signature couldn't be read: {}", e.message()),
                };
            }
        }
        // The signer's certificate even when the check failed, to say who it claims to be.
        let store = unsafe { CryptGetMessageCertificates(ENC.0, None, 0, cms) }.ok();
        if signer.is_null() {
            if let Some(st) = store {
                signer = signer_in(cms, st);
            }
        }
        if signer.is_null() {
            if let Some(st) = store {
                unsafe {
                    let _ = CertCloseStore(Some(st), 0);
                }
            }
            if !out.intact {
                out.trust = "invalid".into();
            }
            return out;
        }
        let s = summary(signer);
        out.signer = s.name;
        out.issuer = s.issuer;
        out.email = s.email;
        out.not_after = s.not_after;
        if !out.intact {
            out.trust = "invalid".into();
        } else {
            if let Some((t, by, trusted)) = timestamp_of(cms) {
                out.timestamp = t;
                out.tsa = by;
                out.timestamp_trusted = trusted;
            }
            // A trusted timestamp says when it was signed; otherwise the time the signature claims.
            let at = if out.timestamp_trusted { Some(out.timestamp) } else { signed_at };
            let (code, _) = chain_status(signer, store, None);
            let (trust, detail) = match code {
                0 => ("trusted", String::new()),
                0x800B_0101 => match at.map(|t| chain_status(signer, store, Some(t)).0) {
                    Some(0) if out.timestamp_trusted => ("expired", "The certificate was valid when it signed (a trusted timestamp proves when) and has expired since.".into()),
                    Some(0) => ("expired", "The certificate was valid when it signed and has expired since.".into()),
                    _ => ("invalid", "The certificate had expired when it signed.".into()),
                },
                0x800B_0109 => ("untrusted", "Windows doesn't trust the authority that issued this certificate (for example a self-signed one).".into()),
                0x800B_010A => ("incomplete", "The certificate of the authority that issued it isn't available, so it can't be checked.".into()),
                0x800B_010C => ("revoked", "The certificate was revoked by its authority.".into()),
                c => ("unknown", format!("Windows couldn't check the certificate (0x{c:08X}).")),
            };
            out.trust = trust.into();
            out.detail = detail;
        }
        unsafe {
            let _ = CertFreeCertificateContext(Some(signer));
            if let Some(st) = store {
                let _ = CertCloseStore(Some(st), 0);
            }
        }
        out
    }

    /// The signer's certificate among those in the message (when the check failed early).
    fn signer_in(cms: &[u8], store: HCERTSTORE) -> *mut CERT_CONTEXT {
        unsafe {
            let msg = CryptMsgOpenToDecode(ENC.0, CMSG_DETACHED_FLAG, 0, None, None, None);
            if msg.is_null() {
                return std::ptr::null_mut();
            }
            let mut found = std::ptr::null_mut();
            if CryptMsgUpdate(msg, Some(cms), true).is_ok() {
                let mut len = 0u32;
                if CryptMsgGetParam(msg, CMSG_SIGNER_CERT_INFO_PARAM, 0, None, &mut len).is_ok() {
                    let mut buf = vec![0u64; (len as usize).div_ceil(8)];
                    if CryptMsgGetParam(msg, CMSG_SIGNER_CERT_INFO_PARAM, 0, Some(buf.as_mut_ptr() as *mut c_void), &mut len).is_ok() {
                        found = CertGetSubjectCertificateFromStore(store, ENC, buf.as_ptr() as *const CERT_INFO);
                    }
                }
            }
            let _ = CryptMsgClose(Some(msg));
            found
        }
    }

    /// Windows' verdict on a certificate's chain (0 = fine), at `at` or now.
    fn chain_status(c: *const CERT_CONTEXT, extra: Option<HCERTSTORE>, at: Option<i64>) -> (u32, ()) {
        let para = CERT_CHAIN_PARA { cbSize: std::mem::size_of::<CERT_CHAIN_PARA>() as u32, ..Default::default() };
        let ft = at.map(unix_to_filetime);
        let mut chain: *mut CERT_CHAIN_CONTEXT = std::ptr::null_mut();
        let ok = unsafe { CertGetCertificateChain(None, c, ft.as_ref().map(|f| f as *const FILETIME), extra, &para, 0, None, &mut chain) };
        if ok.is_err() || chain.is_null() {
            return (0x800B_010A, ());
        }
        let policy = CERT_CHAIN_POLICY_PARA { cbSize: std::mem::size_of::<CERT_CHAIN_POLICY_PARA>() as u32, ..Default::default() };
        let mut status = CERT_CHAIN_POLICY_STATUS { cbSize: std::mem::size_of::<CERT_CHAIN_POLICY_STATUS>() as u32, ..Default::default() };
        let ran = unsafe { CertVerifyCertificateChainPolicy(CERT_CHAIN_POLICY_BASE, chain, &policy, &mut status) };
        unsafe { CertFreeCertificateChain(chain) };
        if !ran.as_bool() {
            return (0x800B_010A, ());
        }
        (status.dwError, ())
    }
}

/// Linux: OpenSSL (the system's libssl). Certificates come from .pfx/.p12 files and from DSC tokens and smart
/// cards through their PKCS#11 drivers (pkcs11.rs); the signature is built the same way as on Windows (CMS
/// SignedData, detached, SHA-256, the chain, signing-certificate-v2) and checked against the system's trusted
/// certificates (/etc/ssl/certs). Timestamps (RFC 3161) and revocation (OCSP) are asked for over the network
/// as on Windows.
#[cfg(not(windows))]
mod imp {
    use super::{CertSummary, Revocation, SigCheck};
    use foreign_types::ForeignTypeRef;
    use openssl::asn1::{Asn1Time, Asn1TimeRef};
    use openssl::hash::{hash, MessageDigest};
    use openssl::nid::Nid;
    use openssl::ocsp::{OcspCertId, OcspCertStatus, OcspRequest, OcspResponse, OcspResponseStatus};
    use openssl::pkcs12::Pkcs12;
    use openssl::pkcs7::{Pkcs7, Pkcs7Flags};
    use openssl::pkey::{PKey, Private};
    use openssl::stack::Stack;
    use openssl::x509::store::{X509Store, X509StoreBuilder};
    use openssl::x509::verify::X509VerifyParam;
    use openssl::x509::{X509NameRef, X509Ref, X509};
    use std::ffi::{c_char, c_int, c_long, c_uint, c_void};

    const OID_SIGNING_CERT_V2: &[u8] = b"1.2.840.113549.1.9.16.2.47\0";
    const OID_TIMESTAMP_TOKEN: &[u8] = b"1.2.840.113549.1.9.16.2.14\0";
    // openssl/cms.h
    const CMS_DETACHED: c_uint = 0x40;
    const CMS_BINARY: c_uint = 0x80;
    const CMS_NOSMIMECAP: c_uint = 0x200;
    const CMS_PARTIAL: c_uint = 0x4000;
    const V_ASN1_SEQUENCE: c_int = 16;

    // The parts of OpenSSL's CMS the openssl crate doesn't cover (in libcrypto, linked through openssl-sys).
    extern "C" {
        fn CMS_sign(signcert: *mut c_void, pkey: *mut c_void, certs: *mut c_void, data: *mut c_void, flags: c_uint) -> *mut c_void;
        fn CMS_add1_signer(cms: *mut c_void, signer: *mut c_void, pk: *mut c_void, md: *const c_void, flags: c_uint) -> *mut c_void;
        fn CMS_add1_cert(cms: *mut c_void, cert: *mut c_void) -> c_int;
        fn CMS_signed_add1_attr_by_OBJ(si: *mut c_void, obj: *const c_void, kind: c_int, bytes: *const c_void, len: c_int) -> c_int;
        fn CMS_unsigned_add1_attr_by_OBJ(si: *mut c_void, obj: *const c_void, kind: c_int, bytes: *const c_void, len: c_int) -> c_int;
        fn CMS_final(cms: *mut c_void, data: *mut c_void, dcont: *mut c_void, flags: c_uint) -> c_int;
        fn CMS_get0_SignerInfos(cms: *mut c_void) -> *mut c_void;
        fn CMS_SignerInfo_get0_signature(si: *mut c_void) -> *mut c_void;
        fn i2d_CMS_ContentInfo(cms: *mut c_void, out: *mut *mut u8) -> c_int;
        fn d2i_CMS_ContentInfo(a: *mut *mut c_void, pp: *mut *const u8, len: c_long) -> *mut c_void;
        fn CMS_ContentInfo_free(cms: *mut c_void);
        fn OPENSSL_sk_value(st: *const c_void, i: c_int) -> *mut c_void;
        fn OPENSSL_sk_num(st: *const c_void) -> c_int;
        fn OBJ_txt2obj(s: *const c_char, no_name: c_int) -> *mut c_void;
        fn OBJ_txt2nid(s: *const c_char) -> c_int;
        fn ASN1_OBJECT_free(o: *mut c_void);
        fn ASN1_STRING_get0_data(x: *const c_void) -> *const u8;
        fn ASN1_STRING_length(x: *const c_void) -> c_int;
        fn BIO_new_mem_buf(buf: *const c_void, len: c_int) -> *mut c_void;
        fn BIO_free(b: *mut c_void) -> c_int;
        fn EVP_sha256() -> *const c_void;
        fn PKCS7_get_signer_info(p7: *mut c_void) -> *mut c_void;
        fn PKCS7_get_attribute(si: *mut c_void, nid: c_int) -> *mut c_void;
        fn i2d_ASN1_TYPE(a: *mut c_void, out: *mut *mut u8) -> c_int;
    }

    fn err(e: openssl::error::ErrorStack) -> String {
        e.to_string()
    }

    /// A DER value: tag, length, content.
    fn der(tag: u8, content: &[u8]) -> Vec<u8> {
        let mut out = vec![tag];
        let n = content.len();
        if n < 0x80 {
            out.push(n as u8);
        } else {
            let bytes: Vec<u8> = n.to_be_bytes().iter().copied().skip_while(|&b| b == 0).collect();
            out.push(0x80 | bytes.len() as u8);
            out.extend(bytes);
        }
        out.extend_from_slice(content);
        out
    }

    /// A DER value at the start of `b`: (tag, header length, content length).
    fn tlv(b: &[u8]) -> Option<(u8, usize, usize)> {
        let (&tag, rest) = b.split_first()?;
        let (&l, rest) = rest.split_first()?;
        if l < 0x80 {
            return Some((tag, 2, l as usize));
        }
        let n = (l & 0x7f) as usize;
        if n == 0 || n > 4 || rest.len() < n {
            return None;
        }
        Some((tag, 2 + n, rest[..n].iter().fold(0usize, |a, &x| (a << 8) | x as usize)))
    }

    /// The DER value at the start of `b` alone (a PDF's /Contents is padded with zeros).
    fn der_len(b: &[u8]) -> usize {
        tlv(b).map_or(b.len(), |(_, h, l)| (h + l).min(b.len()))
    }

    /// Bytes an OpenSSL `i2d_` function writes.
    unsafe fn i2d(f: unsafe extern "C" fn(*mut c_void, *mut *mut u8) -> c_int, x: *mut c_void) -> Result<Vec<u8>, String> {
        let n = f(x, std::ptr::null_mut());
        if n <= 0 {
            return Err("couldn't encode the signature".into());
        }
        let mut out = vec![0u8; n as usize];
        let mut p = out.as_mut_ptr();
        f(x, &mut p);
        Ok(out)
    }

    fn name(n: &X509NameRef, nid: Nid) -> String {
        n.entries_by_nid(nid).next().and_then(|e| e.data().to_string().ok()).unwrap_or_default()
    }

    fn epoch(t: &Asn1TimeRef) -> i64 {
        Asn1Time::from_unix(0).ok().and_then(|z| z.diff(t).ok()).map_or(0, |d| d.days as i64 * 86_400 + d.secs as i64)
    }

    fn now() -> i64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
    }

    fn email(c: &X509Ref) -> String {
        let from_name = name(c.subject_name(), Nid::PKCS9_EMAILADDRESS);
        if !from_name.is_empty() {
            return from_name;
        }
        c.subject_alt_names().and_then(|s| s.iter().find_map(|g| g.email().map(str::to_string))).unwrap_or_default()
    }

    fn summarize(c: &X509Ref, provider: &str, hardware: bool) -> CertSummary {
        let thumb = c.digest(MessageDigest::sha1()).map(|d| d.iter().map(|b| format!("{b:02X}")).collect()).unwrap_or_default();
        let (not_before, not_after) = (epoch(c.not_before()), epoch(c.not_after()));
        let t = now();
        let self_signed = c.subject_name().to_der().ok() == c.issuer_name().to_der().ok();
        CertSummary {
            thumb,
            name: name(c.subject_name(), Nid::COMMONNAME),
            issuer: name(c.issuer_name(), Nid::COMMONNAME),
            email: email(c),
            org: name(c.subject_name(), Nid::ORGANIZATIONNAME),
            not_before,
            not_after,
            provider: provider.into(),
            hardware,
            current: not_before <= t && t <= not_after,
            likely: !self_signed,
        }
    }

    /// The system's trusted certificates, checked as at `at` (seconds since 1970) when given.
    fn trusted(at: Option<i64>) -> Result<X509Store, String> {
        let mut b = X509StoreBuilder::new().map_err(err)?;
        b.set_default_paths().map_err(err)?;
        if let Some(t) = at {
            let mut p = X509VerifyParam::new().map_err(err)?;
            p.set_time(t as _);
            b.set_param(&p).map_err(err)?;
        }
        Ok(b.build())
    }

    /// The certificates on the DSC tokens and smart cards plugged in (certificate files are opened one by one).
    pub fn list() -> Result<Vec<CertSummary>, String> {
        Ok(crate::pkcs11::certificates().iter().map(|t| summarize(&t.cert, &t.label, true)).collect())
    }

    /// Where a certificate's key is: in memory (a certificate file), or on a token (with the PIN to give it).
    enum Key {
        File(PKey<Private>),
        Token(crate::pkcs11::TokenCert, Option<String>),
    }

    pub struct Cert {
        cert: X509,
        key: Key,
        chain: Vec<X509>,
    }

    impl Cert {
        pub fn find(thumb: &str) -> Result<Cert, String> {
            let t = crate::pkcs11::certificates()
                .into_iter()
                .find(|t| summarize(&t.cert, "", true).thumb.eq_ignore_ascii_case(thumb))
                .ok_or("The certificate's token isn't plugged in (or its driver isn't installed): plug it in and press Refresh.")?;
            // The token's own copies of the issuers, up the chain.
            let mut chain: Vec<X509> = Vec::new();
            let mut at = t.cert.clone();
            while let Some(up) = t.others.iter().find(|o| o.subject_name().to_der().ok() == at.issuer_name().to_der().ok() && o.to_der().ok() != at.to_der().ok()) {
                if chain.iter().any(|c| c.to_der().ok() == up.to_der().ok()) {
                    break;
                }
                chain.push(up.clone());
                at = up.clone();
            }
            Ok(Cert { cert: t.cert.clone(), key: Key::Token(t, None), chain })
        }

        /// The token's PIN, for the signature about to be made.
        pub fn set_pin(&mut self, pin: Option<String>) {
            if let Key::Token(_, p) = &mut self.key {
                *p = pin;
            }
        }

        pub fn from_pfx(path: &str, password: &str) -> Result<Cert, String> {
            let der = std::fs::read(path).map_err(|e| format!("Couldn't read the certificate file: {e}"))?;
            let p12 = Pkcs12::from_der(&der).map_err(|_| "This isn't a certificate file (.pfx or .p12).".to_string())?;
            let parsed = p12.parse2(password).map_err(|_| "Wrong password for the certificate file.".to_string())?;
            let cert = parsed.cert.ok_or("The file has no certificate.")?;
            let key = parsed.pkey.ok_or("The file has no private key: it can't sign.")?;
            let chain = parsed.ca.map(|s| s.into_iter().collect()).unwrap_or_default();
            Ok(Cert { cert, key: Key::File(key), chain })
        }

        pub fn summary(&self) -> CertSummary {
            match &self.key {
                Key::File(_) => summarize(&self.cert, "Certificate file", false),
                Key::Token(t, _) => summarize(&self.cert, &t.label, true),
            }
        }

        pub fn dn(&self) -> [String; 5] {
            let n = self.cert.subject_name();
            [name(n, Nid::COMMONNAME), name(n, Nid::ORGANIZATIONNAME), name(n, Nid::ORGANIZATIONALUNITNAME), email(&self.cert), name(n, Nid::COUNTRYNAME)]
        }

        /// The CMS signature over `data`.
        pub fn sign(&self, data: &[u8], _hwnd: isize) -> Result<Vec<u8>, String> {
            let cert_hash = hash(MessageDigest::sha256(), &self.cert.to_der().map_err(err)?).map_err(err)?;
            // SigningCertificateV2 { [ESSCertIDv2 { certHash }] } (SHA-256, the default, so no algorithm).
            let scv2 = der(0x30, &der(0x30, &der(0x30, &der(0x04, &cert_hash))));
            let key = match &self.key {
                Key::File(k) => k,
                Key::Token(t, pin) => return self.sign_on_token(t, pin.as_deref(), data, scv2),
            };
            // SAFETY: OpenSSL objects made and freed here; the certificate and key outlive the calls (OpenSSL
            // takes its own references to them).
            unsafe {
                let cms = CMS_sign(std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), CMS_DETACHED | CMS_BINARY | CMS_PARTIAL);
                if cms.is_null() {
                    return Err("Couldn't start the signature.".into());
                }
                let r = (|| {
                    let si = CMS_add1_signer(cms, self.cert.as_ptr() as *mut c_void, key.as_ptr() as *mut c_void, EVP_sha256(), CMS_BINARY | CMS_NOSMIMECAP);
                    if si.is_null() {
                        return Err("The certificate's key couldn't sign.".to_string());
                    }
                    let obj = OBJ_txt2obj(OID_SIGNING_CERT_V2.as_ptr().cast(), 1);
                    let ok = CMS_signed_add1_attr_by_OBJ(si, obj, V_ASN1_SEQUENCE, scv2.as_ptr().cast(), scv2.len() as c_int);
                    ASN1_OBJECT_free(obj);
                    if ok != 1 {
                        return Err("Couldn't add the certificate to the signature.".into());
                    }
                    for c in &self.chain {
                        CMS_add1_cert(cms, c.as_ptr() as *mut c_void);
                    }
                    let bio = BIO_new_mem_buf(data.as_ptr().cast(), data.len() as c_int);
                    let ok = CMS_final(cms, bio, std::ptr::null_mut(), CMS_DETACHED | CMS_BINARY);
                    BIO_free(bio);
                    if ok != 1 {
                        return Err(format!("Couldn't sign: {}", openssl::error::ErrorStack::get()));
                    }
                    i2d(i2d_CMS_ContentInfo, cms)
                })();
                CMS_ContentInfo_free(cms);
                r
            }
        }

        /// The CMS put together here around the token's signature of its signed attributes (content type,
        /// signing time, message digest, signing-certificate-v2): OpenSSL can't use a key it can't hold.
        fn sign_on_token(&self, t: &crate::pkcs11::TokenCert, pin: Option<&str>, data: &[u8], scv2: Vec<u8>) -> Result<Vec<u8>, String> {
            const OID_DATA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x01];
            const OID_SIGNED_DATA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x02];
            const OID_CONTENT_TYPE: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x09, 0x03];
            const OID_MESSAGE_DIGEST: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x09, 0x04];
            const OID_SIGNING_TIME: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x09, 0x05];
            const OID_SCV2: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x09, 0x10, 0x02, 0x2f];
            const OID_SHA256: &[u8] = &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01];
            const OID_RSA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];
            const OID_ECDSA_SHA256: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02];
            let oid = |o: &[u8]| der(0x06, o);
            let attr = |o: &[u8], value: Vec<u8>| der(0x30, &[oid(o), der(0x31, &value)].concat());
            let digest = hash(MessageDigest::sha256(), data).map_err(err)?;
            let mut attrs = vec![
                attr(OID_CONTENT_TYPE, oid(OID_DATA)),
                attr(OID_SIGNING_TIME, der(0x17, utc_time(now()).as_bytes())),
                attr(OID_MESSAGE_DIGEST, der(0x04, &digest)),
                attr(OID_SCV2, scv2),
            ];
            // A DER SET OF is sorted by its members' encodings.
            attrs.sort();
            let attrs = attrs.concat();
            let (sig, ec) = crate::pkcs11::sign(t, pin, &der(0x31, &attrs))?;
            let sha256 = der(0x30, &[oid(OID_SHA256), vec![0x05, 0x00]].concat());
            let sig_alg = if ec { der(0x30, &oid(OID_ECDSA_SHA256)) } else { der(0x30, &[oid(OID_RSA), vec![0x05, 0x00]].concat()) };
            let serial = self.cert.serial_number().to_bn().map_err(err)?.to_vec();
            let serial = if serial.first().map_or(true, |&b| b & 0x80 != 0) { [&[0u8][..], &serial].concat() } else { serial };
            let issuer_serial = der(0x30, &[self.cert.issuer_name().to_der().map_err(err)?, der(0x02, &serial)].concat());
            let signer_info = der(0x30, &[der(0x02, &[1]), issuer_serial, sha256.clone(), der(0xa0, &attrs), sig_alg, der(0x04, &sig)].concat());
            let mut certs = vec![self.cert.to_der().map_err(err)?];
            for c in &self.chain {
                certs.push(c.to_der().map_err(err)?);
            }
            let signed_data = der(
                0x30,
                &[der(0x02, &[1]), der(0x31, &sha256), der(0x30, &oid(OID_DATA)), der(0xa0, &certs.concat()), der(0x31, &signer_info)].concat(),
            );
            Ok(der(0x30, &[oid(OID_SIGNED_DATA), der(0xa0, &signed_data)].concat()))
        }
    }

    /// UTCTime for `t` (seconds since 1970): YYMMDDHHMMSSZ.
    fn utc_time(t: i64) -> String {
        let (days, secs) = (t.div_euclid(86_400), t.rem_euclid(86_400));
        // Civil date from days since 1970 (Howard Hinnant's algorithm).
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
        format!("{:02}{m:02}{d:02}{:02}{:02}{:02}Z", y % 100, secs / 3600, secs % 3600 / 60, secs % 60)
    }

    /// The timestamp token in a signature's unsigned attributes (DER), if any.
    fn timestamp_token(p7: &Pkcs7) -> Option<Vec<u8>> {
        // SAFETY: borrowed pointers into `p7`, read only while it lives.
        unsafe {
            let sis = PKCS7_get_signer_info(p7.as_ptr() as *mut c_void);
            if sis.is_null() || OPENSSL_sk_num(sis) < 1 {
                return None;
            }
            let nid = OBJ_txt2nid(OID_TIMESTAMP_TOKEN.as_ptr().cast());
            let t = PKCS7_get_attribute(OPENSSL_sk_value(sis, 0), nid);
            if t.is_null() {
                None
            } else {
                i2d(i2d_ASN1_TYPE, t).ok()
            }
        }
    }

    /// Days since 1970 of a civil date (Howard Hinnant's algorithm).
    fn days(y: i64, m: i64, d: i64) -> i64 {
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// A timestamp token's time: the first GeneralizedTime in it is TSTInfo's genTime (the content comes before
    /// the TSA's certificates and signer info).
    fn gen_time(token: &[u8]) -> i64 {
        for i in 0..token.len().saturating_sub(17) {
            let n = token[i + 1] as usize;
            if token[i] != 0x18 || !(15..=24).contains(&n) || i + 2 + n > token.len() {
                continue;
            }
            let t = &token[i + 2..i + 2 + n];
            if t.last() != Some(&b'Z') || !t[..14].iter().all(u8::is_ascii_digit) {
                continue;
            }
            let num = |a: usize, b: usize| std::str::from_utf8(&t[a..b]).ok().and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
            return days(num(0, 4), num(4, 6), num(6, 8)) * 86_400 + num(8, 10) * 3600 + num(10, 12) * 60 + num(12, 14);
        }
        0
    }

    /// Checks a signature: that `data` is what was signed, who signed it, and whether the system trusts the
    /// certificate (at signing time if it has expired since).
    pub fn verify(data: &[u8], cms: &[u8], signed_at: Option<i64>) -> SigCheck {
        let cms = &cms[..der_len(cms)];
        let mut out = SigCheck { trust: "unknown".into(), ..Default::default() };
        let Ok(p7) = Pkcs7::from_der(cms) else {
            out.trust = "invalid".into();
            out.detail = "The signature can't be read.".into();
            return out;
        };
        let Ok(none) = Stack::<X509>::new() else { return out };
        let Ok(empty) = X509StoreBuilder::new().map(|b| b.build()) else { return out };
        out.intact = p7.verify(&none, &empty, Some(data), None, Pkcs7Flags::BINARY | Pkcs7Flags::NOVERIFY).is_ok();
        if let Some(signer) = p7.signers(&none, Pkcs7Flags::empty()).ok().and_then(|s| s.into_iter().next()) {
            out.signer = name(signer.subject_name(), Nid::COMMONNAME);
            out.issuer = name(signer.issuer_name(), Nid::COMMONNAME);
            out.email = email(&signer);
            out.not_after = epoch(signer.not_after());
        }
        if let Some(token) = timestamp_token(&p7) {
            out.timestamp = gen_time(&token);
            if let Ok(t) = Pkcs7::from_der(&token) {
                if let Some(tsa) = t.signers(&none, Pkcs7Flags::empty()).ok().and_then(|s| s.into_iter().next()) {
                    out.tsa = name(tsa.subject_name(), Nid::COMMONNAME);
                }
                out.timestamp_trusted = trusted(None).is_ok_and(|store| t.verify(&none, &store, None, None, Pkcs7Flags::BINARY).is_ok());
            }
        }
        if !out.intact {
            out.trust = "invalid".into();
            out.detail = "The document was changed after it was signed, or the signature is damaged.".into();
            return out;
        }
        let at = (out.timestamp > 0).then_some(out.timestamp).or(signed_at);
        let Ok(store) = trusted(at) else { return out };
        match p7.verify(&none, &store, Some(data), None, Pkcs7Flags::BINARY) {
            Ok(()) if out.not_after > 0 && now() > out.not_after => {
                out.trust = "expired".into();
                out.detail = "The certificate was valid when it signed, and has expired since.".into();
            }
            Ok(()) => out.trust = "trusted".into(),
            Err(e) => {
                let why = e.to_string();
                (out.trust, out.detail) = if why.contains("expired") {
                    ("expired".into(), "The certificate has expired.".into())
                } else if why.contains("revoked") {
                    ("revoked".into(), "The certificate was revoked by its issuer.".into())
                } else if why.contains("self-signed") || why.contains("self signed") {
                    ("untrusted".into(), "The certificate's authority isn't one this system trusts.".into())
                } else if why.contains("unable to get") {
                    ("incomplete".into(), "The certificate that issued the signer's isn't in the signature or this system.".into())
                } else {
                    ("untrusted".into(), why)
                };
            }
        }
        out
    }

    fn post(url: &str, kind: &str, body: &[u8]) -> Result<Vec<u8>, String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .tls_config(ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::PlatformVerifier).build())
            .proxy(ureq::Proxy::try_from_env())
            .http_status_as_error(false)
            .timeout_global(Some(std::time::Duration::from_secs(30)))
            .user_agent("Paginaut")
            .build()
            .into();
        let mut resp = agent.post(url).header("Content-Type", kind).send(body).map_err(|e| format!("{url} didn't answer: {e}"))?;
        if resp.status().as_u16() != 200 {
            return Err(format!("{url} answered {}.", resp.status().as_u16()));
        }
        resp.body_mut().with_config().limit(1 << 20).read_to_vec().map_err(|e| e.to_string())
    }

    /// Adds a trusted timestamp (RFC 3161) from the service at `url` to the signature. Returns the new
    /// signature and the time.
    pub fn add_timestamp(cms: &[u8], url: &str) -> Result<(Vec<u8>, i64), String> {
        let cms = &cms[..der_len(cms)];
        // SAFETY: the CMS object is parsed, changed and freed here; borrowed pointers are read while it lives.
        unsafe {
            let mut p = cms.as_ptr();
            let ci = d2i_CMS_ContentInfo(std::ptr::null_mut(), &mut p, cms.len() as c_long);
            if ci.is_null() {
                return Err("The signature can't be read.".into());
            }
            let r = (|| {
                let sis = CMS_get0_SignerInfos(ci);
                if sis.is_null() || OPENSSL_sk_num(sis) < 1 {
                    return Err("The signature has no signer.".to_string());
                }
                let si = OPENSSL_sk_value(sis, 0);
                let sig = CMS_SignerInfo_get0_signature(si);
                let value = std::slice::from_raw_parts(ASN1_STRING_get0_data(sig), ASN1_STRING_length(sig) as usize);
                let imprint = hash(MessageDigest::sha256(), value).map_err(err)?;
                let mut nonce = [0u8; 8];
                openssl::rand::rand_bytes(&mut nonce).map_err(err)?;
                nonce[0] &= 0x7f;
                // TimeStampReq { version 1, messageImprint { sha256, imprint }, nonce, certReq TRUE }
                let sha256 = der(0x30, &[der(0x06, &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01]), vec![0x05, 0x00]].concat());
                let req = der(0x30, &[der(0x02, &[1]), der(0x30, &[sha256, der(0x04, &imprint)].concat()), der(0x02, &nonce), vec![0x01, 0x01, 0xff]].concat());
                let resp = post(url, "application/timestamp-query", &req)?;
                // TimeStampResp { status PKIStatusInfo, timeStampToken ContentInfo }
                let (_, h, _) = tlv(&resp).ok_or("The timestamp service's answer can't be read.")?;
                let body = &resp[h..];
                let (_, sh, sl) = tlv(body).ok_or("The timestamp service's answer can't be read.")?;
                let status = body.get(sh + 2).copied().unwrap_or(2);
                if status > 1 {
                    return Err(format!("The timestamp service ({url}) refused the request."));
                }
                let token = &body[sh + sl..];
                let token = &token[..der_len(token)];
                if token.is_empty() {
                    return Err("The timestamp service sent no timestamp.".into());
                }
                let obj = OBJ_txt2obj(OID_TIMESTAMP_TOKEN.as_ptr().cast(), 1);
                let ok = CMS_unsigned_add1_attr_by_OBJ(si, obj, V_ASN1_SEQUENCE, token.as_ptr().cast(), token.len() as c_int);
                ASN1_OBJECT_free(obj);
                if ok != 1 {
                    return Err("Couldn't add the timestamp to the signature.".into());
                }
                Ok((i2d(i2d_CMS_ContentInfo, ci)?, gen_time(token)))
            })();
            CMS_ContentInfo_free(ci);
            r
        }
    }

    /// Asks the signer certificate's issuer (OCSP) whether it has been revoked.
    pub fn revocation(cms: &[u8]) -> Revocation {
        let unknown = |detail: String| Revocation { status: "unknown".into(), detail };
        let cms = &cms[..der_len(cms)];
        let Ok(p7) = Pkcs7::from_der(cms) else { return unknown("The signature can't be read.".into()) };
        let Ok(none) = Stack::<X509>::new() else { return unknown(String::new()) };
        let Some(signer) = p7.signers(&none, Pkcs7Flags::empty()).ok().and_then(|s| s.into_iter().next()) else {
            return unknown("The signature has no certificate.".into());
        };
        let certs: Vec<X509> = p7.signed().and_then(|s| s.certificates()).map(|c| c.iter().map(|x| x.to_owned()).collect()).unwrap_or_default();
        let issuer_name = signer.issuer_name().to_der().ok();
        let Some(issuer) = certs.iter().find(|c| c.subject_name().to_der().ok() == issuer_name) else {
            return unknown("The certificate that issued the signer's isn't in the signature.".into());
        };
        let Some(url) = signer.ocsp_responders().ok().and_then(|r| r.iter().next().map(|u| u.to_string())) else {
            return unknown("The issuer publishes no online revocation check.".into());
        };
        let check = || -> Result<Revocation, String> {
            let mut req = OcspRequest::new().map_err(err)?;
            req.add_id(OcspCertId::from_cert(MessageDigest::sha1(), &signer, issuer).map_err(err)?).map_err(err)?;
            let resp = OcspResponse::from_der(&post(&url, "application/ocsp-request", &req.to_der().map_err(err)?)?).map_err(err)?;
            if resp.status() != OcspResponseStatus::SUCCESSFUL {
                return Ok(unknown("The issuer's revocation service couldn't answer.".into()));
            }
            let basic = resp.basic().map_err(err)?;
            let id = OcspCertId::from_cert(MessageDigest::sha1(), &signer, issuer).map_err(err)?;
            Ok(match basic.find_status(&id) {
                Some(s) if s.status == OcspCertStatus::GOOD => Revocation { status: "good".into(), detail: "The issuer says the certificate hasn't been revoked.".into() },
                Some(s) if s.status == OcspCertStatus::REVOKED => Revocation { status: "revoked".into(), detail: "The issuer has revoked the certificate.".into() },
                _ => unknown("The issuer doesn't know this certificate.".into()),
            })
        };
        check().unwrap_or_else(unknown)
    }
}
#[cfg(not(windows))]
pub use imp::*;

/// Linux: signing with a certificate file and checking the result (cargo test --lib dsc:: in WSL or on Linux;
/// makes its own throwaway certificate with the openssl program).
#[cfg(all(test, not(windows)))]
mod linux_tests {
    #[test]
    fn signs_and_checks() {
        let dir = std::env::temp_dir().join("paginaut-dsc-test");
        let _ = std::fs::create_dir_all(&dir);
        let (key, crt, pfx) = (dir.join("k.pem"), dir.join("c.pem"), dir.join("t.p12"));
        let run = |args: &[&str]| assert!(std::process::Command::new("openssl").args(args).status().unwrap().success());
        run(&["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", key.to_str().unwrap(), "-out", crt.to_str().unwrap(), "-days", "30", "-subj", "/CN=Paginaut Test Signer/O=Paginaut/emailAddress=test@example.com"]);
        run(&["pkcs12", "-export", "-inkey", key.to_str().unwrap(), "-in", crt.to_str().unwrap(), "-out", pfx.to_str().unwrap(), "-passout", "pass:secret"]);
        assert!(super::Cert::from_pfx(pfx.to_str().unwrap(), "wrong").is_err());
        let cert = super::Cert::from_pfx(pfx.to_str().unwrap(), "secret").unwrap();
        let s = cert.summary();
        assert_eq!(s.name, "Paginaut Test Signer");
        assert_eq!(s.email, "test@example.com");
        assert!(s.current && !s.likely);
        assert_eq!(cert.dn()[1], "Paginaut");
        let data = b"the bytes a PDF signature covers";
        let mut cms = cert.sign(data, 0).unwrap();
        cms.resize(cms.len() + 64, 0); // /Contents is padded with zeros
        let ok = super::verify(data, &cms, None);
        assert!(ok.intact, "{ok:?}");
        assert_eq!(ok.signer, "Paginaut Test Signer");
        assert_eq!(ok.trust, "untrusted", "{ok:?}"); // self-signed
        let bad = super::verify(b"changed bytes", &cms, None);
        assert!(!bad.intact);
        assert_eq!(bad.trust, "invalid");
        // The signing-certificate-v2 attribute is in: its OID's bytes appear in the signature.
        let oid = [0x06, 0x0b, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x09, 0x10, 0x02, 0x2f];
        assert!(cms.windows(oid.len()).any(|w| w == oid));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Linux: signing with a key on a token, through SoftHSM (a token in software) set up by the test's caller:
/// PAGINAUT_SOFTHSM_CONF=<softhsm2.conf> with a token "Paginaut DSC" (PIN 1234) holding a key and certificate
/// "Token Signer" with the same id.
#[cfg(all(test, not(windows)))]
mod token_tests {
    #[test]
    fn signs_on_a_token() {
        let Ok(conf) = std::env::var("PAGINAUT_SOFTHSM_CONF") else { return };
        std::env::set_var("SOFTHSM2_CONF", conf);
        crate::pkcs11::set_module(Some("/usr/lib/softhsm/libsofthsm2.so".into()));
        let list = super::list().unwrap();
        let c = list.iter().find(|c| c.name == "Token Signer").expect("the token's certificate is listed");
        assert!(c.hardware);
        assert_eq!(c.provider, "Paginaut DSC");
        let mut cert = super::Cert::find(&c.thumb).unwrap();
        let data = b"bytes covered by a token signature";
        cert.set_pin(Some("0000".into()));
        let wrong = cert.sign(data, 0).unwrap_err();
        assert!(wrong.contains("Wrong PIN"), "{wrong}");
        cert.set_pin(Some("1234".into()));
        let cms = cert.sign(data, 0).unwrap();
        let check = super::verify(data, &cms, None);
        assert!(check.intact, "{check:?}");
        assert_eq!(check.signer, "Token Signer");
        assert!(!super::verify(b"other bytes", &cms, None).intact);
    }
}
