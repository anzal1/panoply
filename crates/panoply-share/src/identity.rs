//! Device identity: a self-signed certificate whose SHA-256 is the device fingerprint, and the
//! rustls configurations that use it.
//!
//! The certificate is created once and kept on disk, so the fingerprint is stable across
//! restarts. Peers are not checked against any certificate authority. A connection is bound to a
//! device by comparing the certificate it presents with the fingerprint that device announced.

use std::fs;
use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms, ring};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, ServerConfig, SignatureScheme};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::proto::{hex, is_fingerprint};

/// SHA-256 of a DER certificate, lowercase hex. This is the device fingerprint.
pub fn fingerprint_of(cert_der: &[u8]) -> String {
    hex(&Sha256::digest(cert_der))
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(ring::default_provider())
}

/// A device's certificate and private key.
pub struct Identity {
    cert: CertificateDer<'static>,
    key: PrivatePkcs8KeyDer<'static>,
    fingerprint: String,
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("fingerprint", &self.fingerprint)
            .finish_non_exhaustive()
    }
}

impl Identity {
    /// Make a fresh identity (ECDSA P-256, valid until 2100).
    pub fn generate() -> Result<Self> {
        let (cert, key_pair) = fresh_certificate()?;
        Ok(Self::from_parts(
            cert.der().clone(),
            PrivatePkcs8KeyDer::from(key_pair.serialize_der()),
        ))
    }

    fn from_parts(cert: CertificateDer<'static>, key: PrivatePkcs8KeyDer<'static>) -> Self {
        let fingerprint = fingerprint_of(&cert);
        Identity {
            cert,
            key,
            fingerprint,
        }
    }

    /// Load `cert.pem` and `key.pem` from `dir`, creating them first when missing.
    pub fn load_or_create(dir: &Path) -> Result<Self> {
        let cert_path = dir.join("cert.pem");
        let key_path = dir.join("key.pem");
        if cert_path.exists() && key_path.exists() {
            let cert = CertificateDer::from_pem_file(&cert_path)
                .map_err(|e| Error::Tls(format!("{}: {e}", cert_path.display())))?;
            let key = match PrivateKeyDer::from_pem_file(&key_path)
                .map_err(|e| Error::Tls(format!("{}: {e}", key_path.display())))?
            {
                PrivateKeyDer::Pkcs8(k) => k,
                _ => return Err(Error::Tls("the identity key must be PKCS#8".into())),
            };
            return Ok(Self::from_parts(cert, key));
        }
        fs::create_dir_all(dir)?;
        let (cert, key_pair) = fresh_certificate()?;
        write_private(&key_path, key_pair.serialize_pem().as_bytes())?;
        fs::write(&cert_path, cert.pem())?;
        Ok(Self::from_parts(
            cert.der().clone(),
            PrivatePkcs8KeyDer::from(key_pair.serialize_der()),
        ))
    }

    /// The device fingerprint: SHA-256 of the DER certificate, lowercase hex.
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    /// The DER certificate.
    pub fn cert_der(&self) -> &[u8] {
        &self.cert
    }

    /// TLS server configuration presenting this identity. No client authentication, as in the
    /// LocalSend protocol.
    pub fn server_config(&self) -> Result<Arc<ServerConfig>> {
        let cfg = ServerConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()?
            .with_no_client_auth()
            .with_single_cert(
                vec![self.cert.clone()],
                PrivateKeyDer::Pkcs8(self.key.clone_key()),
            )?;
        Ok(Arc::new(cfg))
    }
}

fn fresh_certificate() -> Result<(rcgen::Certificate, rcgen::KeyPair)> {
    let tls = |e: rcgen::Error| Error::Tls(e.to_string());
    let key_pair = rcgen::KeyPair::generate().map_err(tls)?;
    let mut params =
        rcgen::CertificateParams::new(vec!["panoply.local".to_string()]).map_err(tls)?;
    let mut dn = rcgen::DistinguishedName::new();
    dn.push(rcgen::DnType::CommonName, "Panoply");
    params.distinguished_name = dn;
    params.not_before = rcgen::date_time_ymd(2020, 1, 1);
    params.not_after = rcgen::date_time_ymd(2100, 1, 1);
    let cert = params.self_signed(&key_pair).map_err(tls)?;
    Ok((cert, key_pair))
}

fn write_private(path: &Path, data: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)?.write_all(data)
}

/// Accepts any certificate, but only after checking it against the fingerprint the device
/// announced. Signatures are checked against the certificate's public key, so a peer must hold
/// the matching private key.
#[derive(Debug)]
struct Pinned {
    expected: Option<String>,
    algs: WebPkiSupportedAlgorithms,
    seen: Arc<Mutex<Option<String>>>,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        let fp = fingerprint_of(end_entity);
        // A well formed fingerprint is enforced. Anything else cannot be a certificate hash
        // (LocalSend ignores the field in HTTPS mode), so there is nothing to compare.
        if let Some(want) = &self.expected
            && is_fingerprint(want)
            && !want.eq_ignore_ascii_case(&fp)
        {
            return Err(rustls::Error::General("fingerprint mismatch".into()));
        }
        *self.seen.lock().unwrap() = Some(fp);
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        // Never reached: the client only offers TLS 1.3.
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algs)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        let spki = spki_of_cert(cert)
            .ok_or_else(|| rustls::Error::General("unreadable certificate".into()))?;
        rustls::crypto::verify_tls13_signature_with_raw_key(message, &spki, dss, &self.algs)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algs.supported_schemes()
    }
}

/// A TLS client configuration that pins the peer to `expected` (when it is a well formed
/// fingerprint). The returned cell receives the fingerprint of the certificate actually seen.
pub fn client_config(expected: Option<&str>) -> (Arc<ClientConfig>, Arc<Mutex<Option<String>>>) {
    let p = provider();
    let seen = Arc::new(Mutex::new(None));
    let verifier = Pinned {
        expected: expected.map(str::to_string),
        algs: p.signature_verification_algorithms,
        seen: seen.clone(),
    };
    // TLS 1.3 only on the client: every LocalSend release negotiates it, and it lets the
    // handshake signature be checked against the bare public key (see `spki_of_cert`).
    let cfg = ClientConfig::builder_with_provider(p)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .expect("ring supports TLS 1.3")
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    (Arc::new(cfg), seen)
}

/// Read one DER TLV: returns (tag, content, rest, whole TLV length).
fn tlv(b: &[u8]) -> Option<(u8, &[u8], &[u8], usize)> {
    let tag = *b.first()?;
    let l0 = *b.get(1)?;
    let (len, hdr) = if l0 < 0x80 {
        (l0 as usize, 2)
    } else {
        let n = (l0 & 0x7f) as usize;
        if n == 0 || n > 4 {
            return None;
        }
        let mut len = 0usize;
        for i in 0..n {
            len = (len << 8) | *b.get(2 + i)? as usize;
        }
        (len, 2 + n)
    };
    let end = hdr.checked_add(len)?;
    let content = b.get(hdr..end)?;
    Some((tag, content, &b[end..], end))
}

/// The DER `SubjectPublicKeyInfo` of a certificate. Walking the DER directly keeps the
/// handshake independent of certificate profile details (a CA flag, odd extensions) that a
/// stock LocalSend certificate might carry, since only the key is needed to verify the
/// handshake signature.
fn spki_of_cert(
    cert: &CertificateDer<'_>,
) -> Option<rustls::pki_types::SubjectPublicKeyInfoDer<'static>> {
    let (_, cert_body, _, _) = tlv(cert)?;
    let (_, tbs, _, _) = tlv(cert_body)?;
    let mut rest = tbs;
    // Optional [0] version.
    let (tag, _, after, _) = tlv(rest)?;
    if tag == 0xA0 {
        rest = after;
    }
    // serial, signature algorithm, issuer, validity, subject
    for _ in 0..5 {
        let (_, _, after, _) = tlv(rest)?;
        rest = after;
    }
    let (tag, _, _, whole) = tlv(rest)?;
    if tag != 0x30 {
        return None;
    }
    Some(rustls::pki_types::SubjectPublicKeyInfoDer::from(
        rest[..whole].to_vec(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_sha256_hex_of_der() {
        let id = Identity::generate().unwrap();
        assert!(is_fingerprint(id.fingerprint()));
        assert_eq!(id.fingerprint(), fingerprint_of(id.cert_der()));
    }

    #[test]
    fn identity_is_stable_across_loads() {
        let dir = std::env::temp_dir().join(format!("panoply-share-id-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let a = Identity::load_or_create(&dir).unwrap();
        let b = Identity::load_or_create(&dir).unwrap();
        assert_eq!(a.fingerprint(), b.fingerprint());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dir.join("key.pem"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(
                mode & 0o077,
                0,
                "the private key must not be readable by others"
            );
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn extracts_the_public_key() {
        let id = Identity::generate().unwrap();
        let spki = spki_of_cert(&CertificateDer::from(id.cert_der().to_vec())).unwrap();
        // An ECDSA P-256 SubjectPublicKeyInfo is 91 bytes.
        assert_eq!(spki.as_ref().len(), 91);
        assert_eq!(spki.as_ref()[0], 0x30);
    }
}
