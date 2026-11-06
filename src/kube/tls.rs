//! TLS adjustments applied to a kubeconfig before the client is built.
//!
//! Some kubeconfigs carry the API server's own certificate in
//! `certificate-authority-data` instead of the CA that issued it (some
//! corporate login tools write them this way). `kubectl` accepts these: Go
//! trusts a server certificate that exactly matches one in the CA pool.
//! rustls only uses CAs as trust anchors, so such a "CA" can never verify the
//! server and every connection fails with `UnknownIssuer`. Those entries are
//! dropped here; with nothing left, the client verifies against the OS trust
//! store (`rustls-platform-verifier`), where an organisation's root CA lives.
//! Real CAs and self-signed certificates are kept: rustls anchors on both.

use kube::Config;

/// Drop server certificates from the kubeconfig's CA list (see the module
/// docs). The rest of the list, if any, is still the only trust.
pub fn prepare(config: &mut Config) {
    let Some(roots) = config.root_cert.as_mut() else {
        return;
    };
    let before = roots.len();
    roots.retain(|der| !is_issued_server_certificate(der));
    let dropped = before - roots.len();
    if dropped == 0 {
        return;
    }
    if roots.is_empty() {
        tracing::info!(
            "certificate-authority of {} is the server's own certificate, not a CA; verifying against the OS trust store",
            config.cluster_url
        );
        config.root_cert = None;
    } else {
        tracing::info!(
            "Ignoring {dropped} server certificate(s) in the certificate-authority of {}",
            config.cluster_url
        );
    }
}

/// Whether `der` is a certificate issued by someone else that isn't a CA —
/// a server certificate, which rustls can't use as a trust anchor. `false`
/// when it can't be read, so an unexpected encoding is left to rustls.
fn is_issued_server_certificate(der: &[u8]) -> bool {
    parse(der).is_some_and(|cert| !cert.self_issued && !cert.ca)
}

struct CertFacts {
    /// Issuer and subject are the same name.
    self_issued: bool,
    /// `basicConstraints` says it is a CA.
    ca: bool,
}

/// `basicConstraints` (2.5.29.19).
const BASIC_CONSTRAINTS: &[u8] = &[0x55, 0x1d, 0x13];

/// The two facts needed from an X.509 certificate, read from its DER.
fn parse(der: &[u8]) -> Option<CertFacts> {
    let (cert, _) = tlv(der, 0x30)?;
    let (tbs, _) = tlv(cert, 0x30)?;
    let mut rest = tbs;
    // version [0] is optional
    if rest.first() == Some(&0xa0) {
        rest = tlv(rest, 0xa0)?.1;
    }
    rest = tlv(rest, 0x02)?.1; // serialNumber
    rest = tlv(rest, 0x30)?.1; // signature algorithm
    let (issuer, after_issuer) = tlv(rest, 0x30)?;
    rest = tlv(after_issuer, 0x30)?.1; // validity
    let (subject, after_subject) = tlv(rest, 0x30)?;
    rest = tlv(after_subject, 0x30)?.1; // subjectPublicKeyInfo

    let mut ca = false;
    while !rest.is_empty() {
        let tag = *rest.first()?;
        let (body, next) = tlv(rest, tag)?;
        if tag == 0xa3 {
            let (extensions, _) = tlv(body, 0x30)?;
            ca = basic_constraints_ca(extensions)?;
        }
        rest = next;
    }
    Some(CertFacts {
        self_issued: issuer == subject,
        ca,
    })
}

/// `cA` from the `basicConstraints` extension, `false` without one.
fn basic_constraints_ca(mut extensions: &[u8]) -> Option<bool> {
    while !extensions.is_empty() {
        let (extension, next) = tlv(extensions, 0x30)?;
        extensions = next;
        let (oid, mut rest) = tlv(extension, 0x06)?;
        if oid != BASIC_CONSTRAINTS {
            continue;
        }
        if rest.first() == Some(&0x01) {
            rest = tlv(rest, 0x01)?.1; // critical
        }
        let (value, _) = tlv(rest, 0x04)?;
        let (constraints, _) = tlv(value, 0x30)?;
        return Some(
            constraints.first() == Some(&0x01)
                && tlv(constraints, 0x01)?.0.first().is_some_and(|b| *b != 0),
        );
    }
    Some(false)
}

/// One DER element with tag `tag`: its contents and what follows it.
fn tlv(input: &[u8], tag: u8) -> Option<(&[u8], &[u8])> {
    let (&found, rest) = input.split_first()?;
    if found != tag {
        return None;
    }
    let (&first, rest) = rest.split_first()?;
    let (len, rest) = if first < 0x80 {
        (usize::from(first), rest)
    } else {
        let count = usize::from(first & 0x7f);
        if count == 0 || count > 4 || rest.len() < count {
            return None;
        }
        let (bytes, rest) = rest.split_at(count);
        let len = bytes.iter().fold(0usize, |n, b| (n << 8) | usize::from(*b));
        (len, rest)
    };
    (rest.len() >= len).then(|| rest.split_at(len))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    /// Generated with `openssl req`/`x509`; private keys discarded.
    fn der(name: &str) -> Vec<u8> {
        let pem = match name {
            "ca" => include_str!("../../tests/fixtures/tls/ca.pem"),
            "intermediate" => include_str!("../../tests/fixtures/tls/int.pem"),
            "server" => include_str!("../../tests/fixtures/tls/leaf.pem"),
            "server-no-constraints" => include_str!("../../tests/fixtures/tls/leaf-noext.pem"),
            "self-signed" => include_str!("../../tests/fixtures/tls/selfsigned.pem"),
            _ => unreachable!("unknown fixture {name}"),
        };
        let body: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
        base64::engine::general_purpose::STANDARD
            .decode(body)
            .unwrap()
    }

    fn config(roots: &[&str]) -> Config {
        let mut config = Config::new("https://api.example.com:6443".parse().unwrap());
        config.root_cert = Some(roots.iter().map(|name| der(name)).collect());
        config
    }

    #[test]
    fn reads_whether_a_certificate_is_a_ca_and_who_issued_it() {
        for (name, self_issued, ca) in [
            ("ca", true, true),
            ("intermediate", false, true),
            ("server", false, false),
            ("server-no-constraints", false, false),
            ("self-signed", true, false),
        ] {
            let facts = parse(&der(name)).unwrap();
            assert_eq!((facts.self_issued, facts.ca), (self_issued, ca), "{name}");
        }
        assert!(parse(b"not a certificate").is_none());
    }

    /// The `mdev login` shape: the server's certificate as the only "CA".
    #[test]
    fn a_server_certificate_as_the_only_ca_falls_back_to_the_os_trust_store() {
        for name in ["server", "server-no-constraints"] {
            let mut config = config(&[name]);
            prepare(&mut config);
            assert_eq!(config.root_cert, None, "{name}");
        }
    }

    #[test]
    fn real_cas_and_self_signed_certificates_stay_the_only_trust() {
        for roots in [
            &["ca"][..],
            &["intermediate"],
            &["self-signed"],
            &["ca", "intermediate"],
        ] {
            let mut config = config(roots);
            let expected = config.root_cert.clone();
            prepare(&mut config);
            assert_eq!(config.root_cert, expected, "{roots:?}");
        }
    }

    #[test]
    fn a_server_certificate_next_to_a_ca_is_dropped_and_the_ca_kept() {
        let mut config = config(&["server", "ca"]);
        prepare(&mut config);
        assert_eq!(config.root_cert, Some(vec![der("ca")]));
    }

    #[test]
    fn unreadable_entries_are_left_to_rustls() {
        let mut config = config(&[]);
        config.root_cert = Some(vec![b"garbage".to_vec()]);
        prepare(&mut config);
        assert_eq!(config.root_cert, Some(vec![b"garbage".to_vec()]));
    }
}
