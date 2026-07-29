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
