//! `OpenSSL::X509::Certificate` -- read-only, over a peer certificate a
//! session hands back. zeo ships no certificate ISSUANCE (no `sign`, no
//! builder), so the surface here is what a verifying client reads.

use crate::builtins::arity;
use crate::dispatch::{RObj, RubyObject};
use crate::ext::openssl::{bin_str, str};
use crate::{ClassId, RubyValue};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use zeo_macros::ruby_class;

pub(crate) struct RCert {
    der: Vec<u8>,
    /// The subject DN in OpenSSL's slash-separated one-line form.
    subject: String,
    issuer: String,
    serial: String,
    frozen: AtomicBool,
}

impl RubyObject for RCert {
    fn class_id(&self) -> ClassId {
        zeo_abi::OPENSSL_X509_CERT_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        self.frozen.load(Ordering::Relaxed)
    }
    fn set_frozen(&self) {
        self.frozen.store(true, Ordering::Relaxed)
    }
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, copy_frozen: bool) -> RObj {
        let d = RCert {
            der: self.der.clone(),
            subject: self.subject.clone(),
            issuer: self.issuer.clone(),
            serial: self.serial.clone(),
            frozen: AtomicBool::new(false),
        };
        if copy_frozen {
            d.set_frozen();
        }
        Arc::new(d)
    }
}

/// OpenSSL's one-line DN (`/CN=example.com/O=Example`).
fn one_line(name: &openssl::x509::X509NameRef) -> String {
    name.entries()
        .map(|e| {
            format!(
                "/{}={}",
                e.object().nid().short_name().unwrap_or("?"),
                e.data().to_string().unwrap_or_default()
            )
        })
        .collect()
}

/// RFC 6125 §6.4.3 wildcard matching of ONE label, transcribed from
/// upstream `ssl.rb`'s `verify_wildcard`: at most one `*`, never inside an
/// A-label, and the literal parts must leave at least one character for the
/// wildcard to stand in for.
fn wildcard_label_matches(host_label: &str, san_label: &str) -> bool {
    let parts: Vec<&str> = san_label.split('*').collect();
    if parts.len() > 2 {
        return false;
    }
    if parts.len() == 1 {
        return san_label == host_label;
    }
    if host_label.starts_with("xn--") && san_label != "*" {
        return false;
    }
    parts[0].len() + parts[1].len() < host_label.len()
        && host_label.starts_with(parts[0])
        && host_label.ends_with(parts[1])
}

/// One presented identifier against the reference hostname -- upstream's
/// `verify_hostname`: ASCII only, case-insensitive, and a wildcard only in
/// the left-most label of an equally-long name.
fn hostname_matches(hostname: &str, san: &str) -> bool {
    if !san.is_ascii() || !hostname.is_ascii() {
        return false;
    }
    let san_lower = san.to_lowercase();
    let host_lower = hostname.to_lowercase();
    let san_parts: Vec<&str> = san_lower.split('.').collect();
    if san_parts.len() < 2 {
        return san == hostname;
    }
    let host_parts: Vec<&str> = host_lower.split('.').collect();
    if san_parts.len() != host_parts.len() {
        return false;
    }
    if !wildcard_label_matches(host_parts[0], san_parts[0]) {
        return false;
    }
    san_parts[1..] == host_parts[1..]
}

/// `OpenSSL::SSL.verify_certificate_identity`: subjectAltName dNSName and
/// iPAddress entries decide when present, and only their ABSENCE falls back
/// to the subject's CN -- upstream's `should_verify_common_name` rule.
pub(crate) fn verify_certificate_identity(cert: &openssl::x509::X509Ref, hostname: &str) -> bool {
    let mut should_verify_common_name = true;
    if let Some(names) = cert.subject_alt_names() {
        for name in names {
            if let Some(dns) = name.dnsname() {
                should_verify_common_name = false;
                if hostname_matches(hostname, dns) {
                    return true;
                }
            } else if let Some(ip) = name.ipaddress() {
                should_verify_common_name = false;
                if hostname
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|parsed| ip_octets(&parsed) == ip)
                {
                    return true;
                }
            }
        }
    }
    if should_verify_common_name {
        for entry in cert.subject_name().entries() {
            if entry.object().nid() != openssl::nid::Nid::COMMONNAME {
                continue;
            }
            if let Ok(cn) = entry.data().to_string() {
                if hostname_matches(hostname, &cn) {
                    return true;
                }
            }
        }
    }
    false
}

/// An address in its network byte order (`IPAddr#hton`), which is what a
/// certificate's iPAddress entry carries.
fn ip_octets(addr: &std::net::IpAddr) -> Vec<u8> {
    match addr {
        std::net::IpAddr::V4(v4) => v4.octets().to_vec(),
        std::net::IpAddr::V6(v6) => v6.octets().to_vec(),
    }
}

pub(crate) fn new_cert(cert: &openssl::x509::X509Ref) -> RubyValue {
    RubyValue::Object(Arc::new(RCert {
        der: cert.to_der().unwrap_or_default(),
        subject: one_line(cert.subject_name()),
        issuer: one_line(cert.issuer_name()),
        serial: cert
            .serial_number()
            .to_bn()
            .and_then(|b| b.to_dec_str().map(|s| s.to_string()))
            .unwrap_or_default(),
        frozen: AtomicBool::new(false),
    }))
}

fn cert_of(recv: &RubyValue) -> &RCert {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RCert>()
            .expect("the Certificate table only dispatches on certificate receivers"),
        _ => unreachable!("the Certificate table only dispatches on certificate receivers"),
    }
}

ruby_class! {
    Certificate = zeo_abi::OPENSSL_X509_CERT_CLASS < zeo_abi::OBJECT_CLASS;

    def "to_der" (recv, args, _block) {
        arity!(args, 0);
        Ok(bin_str(cert_of(recv).der.clone()))
    }
    // `subject`/`issuer` answer the DN as a String; CRuby answers an
    // `X509::Name` whose `to_s` is this string (documented divergence).
    def "subject" (recv, args, _block) {
        arity!(args, 0);
        Ok(str(cert_of(recv).subject.clone()))
    }
    def "issuer" (recv, args, _block) {
        arity!(args, 0);
        Ok(str(cert_of(recv).issuer.clone()))
    }
    def "serial" (recv, args, _block) {
        arity!(args, 0);
        Ok(str(cert_of(recv).serial.clone()))
    }
    def "to_s" | "inspect" (recv, args, _block) {
        arity!(args, 0);
        Ok(str(format!(
            "#<OpenSSL::X509::Certificate subject={}>",
            cert_of(recv).subject
        )))
    }
}
