//! `OpenSSL::X509::Store` -- a handle on libssl's own trust store. zeo has
//! no certificate-by-certificate store manipulation (there is no X509
//! issuance surface), so the mutators are accepted and answer self, and
//! what actually decides trust is the owning `SSLContext`'s `ca_file`/
//! default-paths configuration.

use crate::builtins::arity;
use crate::dispatch::{RObj, RubyObject};
use crate::{ClassId, RubyValue};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use zeo_macros::ruby_class;

pub(crate) struct RStore {
    frozen: AtomicBool,
}

impl RubyObject for RStore {
    fn class_id(&self) -> ClassId {
        zeo_abi::OPENSSL_X509_STORE_CLASS
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
        let d = RStore {
            frozen: AtomicBool::new(false),
        };
        if copy_frozen {
            d.set_frozen();
        }
        Arc::new(d)
    }
}

pub(crate) fn new_store() -> RubyValue {
    RubyValue::Object(Arc::new(RStore {
        frozen: AtomicBool::new(false),
    }))
}

ruby_class! {
    Store = zeo_abi::OPENSSL_X509_STORE_CLASS < zeo_abi::OBJECT_CLASS;

    def self."new" arity -1 (_recv, args, _block) {
        arity!(args, 0..=1);
        Ok(new_store())
    }
    def "set_default_paths" (_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Nil)
    }
    def "add_file" (recv, args, _block) {
        arity!(args, 1);
        Ok(recv.clone())
    }
    def "add_path" (recv, args, _block) {
        arity!(args, 1);
        Ok(recv.clone())
    }
}
