//! Digital signatures with certificates from the Windows certificate store: the
//! store is where a DSC token's driver puts its certificates (Adobe finds them
//! there too). Signing goes through Windows' own crypto, so the token's driver
//! asks for its PIN in its own window; Paginaut never sees it.
//!
//! The signature is a CMS (PKCS #7) SignedData, detached, SHA-256, with the
//! certificate chain and the signing-certificate-v2 attribute (as PAdES wants).
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
}

#[cfg(windows)]
pub use imp::*;

#[cfg(windows)]
mod imp {
    use super::{CertSummary, SigCheck};
    use std::ffi::c_void;
    use windows::core::{s, w, PCSTR, PSTR};
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::Security::Cryptography::*;

    const ENC: CERT_QUERY_ENCODING_TYPE = CERT_QUERY_ENCODING_TYPE(X509_ASN_ENCODING.0 | PKCS_7_ASN_ENCODING.0);
    const OID_SHA256: &[u8] = b"2.16.840.1.101.3.4.2.1\0";
    const OID_SHA1: &[u8] = b"1.3.14.3.2.26\0";
    const OID_SIGNING_TIME: &[u8] = b"1.2.840.113549.1.9.5\0";
    const OID_SIGNING_CERT_V2: &[u8] = b"1.2.840.113549.1.9.16.2.47\0";

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
    /// find it (it would go looking for a smart card).
    pub struct Cert(*const CERT_CONTEXT, Option<NCRYPT_KEY_HANDLE>);

    impl Drop for Cert {
        fn drop(&mut self) {
            unsafe {
                let _ = CertFreeCertificateContext(Some(self.0));
                if let Some(k) = self.1 {
                    let _ = NCryptFreeObject(k.into());
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
            Ok(Cert(c, None))
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
            if unsafe { CertGetCertificateChain(None, self.0, None, None, &para, 0, None, &mut chain) }.is_ok() && !chain.is_null() {
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
            Ok(Cert(c, Some(key)))
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
            let (code, _) = chain_status(signer, store, None);
            let (trust, detail) = match code {
                0 => ("trusted", String::new()),
                0x800B_0101 => match signed_at.map(|t| chain_status(signer, store, Some(t)).0) {
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

#[cfg(not(windows))]
mod imp {
    use super::{CertSummary, SigCheck};
    pub struct Cert;
    pub fn list() -> Result<Vec<CertSummary>, String> {
        Ok(Vec::new())
    }
    impl Cert {
        pub fn find(_: &str) -> Result<Cert, String> {
            Err("Signing with certificates is only available on Windows for now.".into())
        }
        pub fn summary(&self) -> CertSummary {
            unreachable!()
        }
        pub fn dn(&self) -> [String; 5] {
            Default::default()
        }
        pub fn sign(&self, _: &[u8], _: isize) -> Result<Vec<u8>, String> {
            Err("not available".into())
        }
    }
    pub fn verify(_: &[u8], _: &[u8], _: Option<i64>) -> SigCheck {
        SigCheck { trust: "unknown".into(), ..Default::default() }
    }
}
#[cfg(not(windows))]
pub use imp::*;
