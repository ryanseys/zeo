//! `OpenSSL::Cipher` -- the EVP symmetric-cipher surface over the vendored
//! libcrypto, driven through the provider-era `CipherCtx` (CRuby's own
//! model: direction first, then key/iv fed into the live context). A
//! legacy-provider algorithm (`RC4`, `BF-CBC`) fails at the EVP fetch and
//! raises `CipherError` at `new` -- exactly where CRuby 4.0's does.
//!
//! `CipherError`/`AuthTagError` live in the gem's Ruby half; `random_key`/
//! `random_iv` (upstream's `cipher.rb` Ruby) are native here so that half
//! stays declaration-only. The `Cipher::AES`/`AES256` shorthand subclasses
//! (upstream `const_set` metaprogramming) are NOT provided -- spell the
//! algorithm out (`Cipher.new("aes-256-cbc")`); see docs/COMPATIBILITY.md.

use super::{bin_str, fill_random, md_from_value, str, str_bytes};
use crate::builtins::{arg_error, arity, convert};
use crate::dispatch::{RObj, RubyObject, raise_error};
use crate::{ClassId, RubyValue, Signal};
use openssl::cipher::Cipher;
use openssl::cipher_ctx::CipherCtx;
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use zeo_macros::ruby_class;

/// The EVP name table, as `OpenSSL::Cipher.ciphers` reports it (CRuby's
/// `EVP_CIPHER_do_all_sorted` walk of the same OpenSSL 3 line). Names, not
/// fetchability -- the legacy-provider entries (`rc4`, `bf`, `idea`) list
/// here yet fail at `new`, as they do under CRuby.
const CIPHER_NAMES: &[&str] = &[
    "aes-128-cbc", "aes-128-ccm", "aes-128-cfb", "aes-128-cfb1", "aes-128-cfb8", "aes-128-ctr",
    "aes-128-ecb", "aes-128-gcm", "aes-128-ocb", "aes-128-ofb", "aes-128-xts", "aes-192-cbc",
    "aes-192-ccm", "aes-192-cfb", "aes-192-cfb1", "aes-192-cfb8", "aes-192-ctr", "aes-192-ecb",
    "aes-192-gcm", "aes-192-ocb", "aes-192-ofb", "aes-256-cbc", "aes-256-ccm", "aes-256-cfb",
    "aes-256-cfb1", "aes-256-cfb8", "aes-256-ctr", "aes-256-ecb", "aes-256-gcm", "aes-256-ocb",
    "aes-256-ofb", "aes-256-xts", "aes128", "aes128-wrap", "aes128-wrap-pad", "aes192",
    "aes192-wrap", "aes192-wrap-pad", "aes256", "aes256-wrap", "aes256-wrap-pad", "aria-128-cbc",
    "aria-128-ccm", "aria-128-cfb", "aria-128-cfb1", "aria-128-cfb8", "aria-128-ctr",
    "aria-128-ecb", "aria-128-gcm", "aria-128-ofb", "aria-192-cbc", "aria-192-ccm",
    "aria-192-cfb", "aria-192-cfb1", "aria-192-cfb8", "aria-192-ctr", "aria-192-ecb",
    "aria-192-gcm", "aria-192-ofb", "aria-256-cbc", "aria-256-ccm", "aria-256-cfb",
    "aria-256-cfb1", "aria-256-cfb8", "aria-256-ctr", "aria-256-ecb", "aria-256-gcm",
    "aria-256-ofb", "aria128", "aria192", "aria256", "bf", "bf-cbc", "bf-cfb", "bf-ecb",
    "bf-ofb", "blowfish", "camellia-128-cbc", "camellia-128-cfb", "camellia-128-cfb1",
    "camellia-128-cfb8", "camellia-128-ctr", "camellia-128-ecb", "camellia-128-ofb",
    "camellia-192-cbc", "camellia-192-cfb", "camellia-192-cfb1", "camellia-192-cfb8",
    "camellia-192-ctr", "camellia-192-ecb", "camellia-192-ofb", "camellia-256-cbc",
    "camellia-256-cfb", "camellia-256-cfb1", "camellia-256-cfb8", "camellia-256-ctr",
    "camellia-256-ecb", "camellia-256-ofb", "camellia128", "camellia192", "camellia256", "cast",
    "cast-cbc", "cast5-cbc", "cast5-cfb", "cast5-ecb", "cast5-ofb", "chacha20",
    "chacha20-poly1305", "des", "des-cbc", "des-cfb", "des-cfb1", "des-cfb8", "des-ecb",
    "des-ede", "des-ede-cbc", "des-ede-cfb", "des-ede-ecb", "des-ede-ofb", "des-ede3",
    "des-ede3-cbc", "des-ede3-cfb", "des-ede3-cfb1", "des-ede3-cfb8", "des-ede3-ecb",
    "des-ede3-ofb", "des-ofb", "des3", "des3-wrap", "desx", "desx-cbc", "id-aes128-CCM",
    "id-aes128-GCM", "id-aes128-wrap", "id-aes128-wrap-pad", "id-aes192-CCM", "id-aes192-GCM",
    "id-aes192-wrap", "id-aes192-wrap-pad", "id-aes256-CCM", "id-aes256-GCM", "id-aes256-wrap",
    "id-aes256-wrap-pad", "id-smime-alg-CMS3DESwrap", "idea", "idea-cbc", "idea-cfb",
    "idea-ecb", "idea-ofb", "rc2", "rc2-128", "rc2-40", "rc2-40-cbc", "rc2-64", "rc2-64-cbc",
    "rc2-cbc", "rc2-cfb", "rc2-ecb", "rc2-ofb", "rc4", "rc4-40", "rc4-hmac-md5", "seed",
    "seed-cbc", "seed-cfb", "seed-ecb", "seed-ofb", "sm4", "sm4-cbc", "sm4-cfb", "sm4-ctr",
    "sm4-ecb", "sm4-ofb",
];

fn cipher_error(msg: String) -> Signal {
    raise_error("OpenSSL::Cipher::CipherError", msg)
}

fn stack_reason(e: &openssl::error::ErrorStack) -> String {
    e.errors()
        .first()
        .and_then(|err| err.reason().map(str::to_string))
        .unwrap_or_else(|| format!("{e}"))
}

struct CState {
    /// The canonical EVP name (`"AES-256-CBC"`), as `#name` reports.
    name: String,
    /// The lower-case fetch spelling, re-fetched on (re)inits.
    fetch_name: String,
    /// `Some(true)` = encrypt, `Some(false)` = decrypt, `None` = unset.
    dir: Option<bool>,
    ctx: Option<CipherCtx>,
    key: Option<Vec<u8>>,
    iv: Option<Vec<u8>>,
    /// An AEAD `iv_len=` override, applied at init before the iv.
    iv_len: Option<usize>,
    padding: bool,
    aead: bool,
}

pub(crate) struct RCipher {
    st: Mutex<CState>,
    frozen: AtomicBool,
}

impl RubyObject for RCipher {
    fn class_id(&self) -> ClassId {
        zeo_abi::OPENSSL_CIPHER_CLASS
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
        // A duplicate restarts from the stored parameters (a live EVP
        // context is not copyable) -- key/iv/direction survive, fed state
        // does not.
        let st = self.st.lock();
        let d = RCipher {
            st: Mutex::new(CState {
                name: st.name.clone(),
                fetch_name: st.fetch_name.clone(),
                dir: st.dir,
                ctx: None,
                key: st.key.clone(),
                iv: st.iv.clone(),
                iv_len: st.iv_len,
                padding: st.padding,
                aead: st.aead,
            }),
            frozen: AtomicBool::new(false),
        };
        if copy_frozen {
            d.set_frozen();
        }
        Arc::new(d)
    }
}

fn cipher_of(recv: &RubyValue) -> &RCipher {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RCipher>()
            .expect("the OpenSSL::Cipher table only dispatches on Cipher receivers"),
        _ => unreachable!("the OpenSSL::Cipher table only dispatches on Cipher receivers"),
    }
}

fn fetch(name: &str) -> Result<Cipher, Signal> {
    Cipher::fetch(None, name, None).map_err(|e| {
        cipher_error(format!(
            "unsupported cipher algorithm: {name}: {}",
            stack_reason(&e)
        ))
    })
}

fn is_aead(lower: &str) -> bool {
    lower.ends_with("gcm")
        || lower.ends_with("ccm")
        || lower.ends_with("ocb")
        || lower == "chacha20-poly1305"
}

/// (Re)build the live context from the stored parameters -- the shared body
/// of `encrypt`/`decrypt`/`reset`, and of the lazy init an early `update`
/// path takes. Encrypt is EVP's own default when no direction was named.
fn rebuild(st: &mut CState) -> Result<(), Signal> {
    let cipher = fetch(&st.fetch_name)?;
    let mut ctx = CipherCtx::new().map_err(|e| cipher_error(stack_reason(&e)))?;
    let init_err = |e: openssl::error::ErrorStack| cipher_error(stack_reason(&e));
    let encrypt = st.dir.unwrap_or(true);
    if encrypt {
        ctx.encrypt_init(Some(&cipher), None, None).map_err(init_err)?;
    } else {
        ctx.decrypt_init(Some(&cipher), None, None).map_err(init_err)?;
    }
    ctx.set_padding(st.padding);
    if let Some(n) = st.iv_len {
        ctx.set_iv_length(n).map_err(init_err)?;
    }
    let (key, iv) = (st.key.clone(), st.iv.clone());
    if key.is_some() || iv.is_some() {
        if encrypt {
            ctx.encrypt_init(None, key.as_deref(), iv.as_deref()).map_err(init_err)?;
        } else {
            ctx.decrypt_init(None, key.as_deref(), iv.as_deref()).map_err(init_err)?;
        }
    }
    st.ctx = Some(ctx);
    Ok(())
}

/// The live context for a data operation; requires the key, CRuby's guard.
fn ready(st: &mut CState) -> Result<&mut CipherCtx, Signal> {
    if st.key.is_none() {
        return Err(cipher_error("key not set".to_string()));
    }
    if st.ctx.is_none() {
        rebuild(st)?;
    }
    Ok(st.ctx.as_mut().expect("rebuild just installed the context"))
}

/// Set a direction: rebuilds the context (EVP_CipherInit), keeping stored
/// key/iv/padding, as CRuby's `#encrypt`/`#decrypt` do.
fn set_dir(recv: &RubyValue, encrypt: bool) -> Result<RubyValue, Signal> {
    let c = cipher_of(recv);
    let mut st = c.st.lock();
    st.dir = Some(encrypt);
    rebuild(&mut st)?;
    Ok(recv.clone())
}

/// Validate-and-store one of the keying parameters, applying it to the live
/// context (building one if a direction is already known).
fn set_param(
    recv: &RubyValue,
    arg: &RubyValue,
    is_key: bool,
) -> Result<RubyValue, Signal> {
    let bytes = str_bytes(arg)?;
    let c = cipher_of(recv);
    let mut st = c.st.lock();
    let cipher = fetch(&st.fetch_name)?;
    if is_key {
        let want = cipher.key_length();
        if bytes.len() != want {
            return Err(arg_error!("key must be {} bytes", want));
        }
        st.key = Some(bytes);
    } else {
        let want = st.iv_len.unwrap_or_else(|| cipher.iv_length());
        if bytes.len() != want {
            return Err(arg_error!("iv must be {} bytes", want));
        }
        st.iv = Some(bytes);
    }
    if st.ctx.is_some() {
        rebuild(&mut st)?;
    }
    Ok(arg.clone())
}

ruby_class! {
    Cipher = zeo_abi::OPENSSL_CIPHER_CLASS < zeo_abi::OBJECT_CLASS;

    // `Cipher.new("aes-256-cbc")` -- any case; `#name` reports the EVP
    // canonical spelling. An unfetchable algorithm raises here.
    def self."new" arity 1 (_recv, args, _block) {
        arity!(args, 1);
        let requested = crate::builtins::convert::to_rstr(&args[0])?
            .lock()
            .to_utf8_lossy()
            .into_owned();
        let lower = requested.to_lowercase();
        let cipher = fetch(&lower)?;
        let name = cipher
            .nid()
            .short_name()
            .map(str::to_string)
            .unwrap_or_else(|_| requested.to_uppercase());
        let aead = is_aead(&lower);
        Ok(RubyValue::Object(Arc::new(RCipher {
            st: Mutex::new(CState {
                name,
                fetch_name: lower,
                dir: None,
                ctx: None,
                key: None,
                iv: None,
                iv_len: None,
                padding: true,
                aead,
            }),
            frozen: AtomicBool::new(false),
        })))
    }
    // The EVP name table (see CIPHER_NAMES).
    def self."ciphers" (_recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Array(crate::array_new(
            CIPHER_NAMES.iter().map(|n| str((*n).to_string())).collect(),
        )))
    }

    def "encrypt" (recv, args, _block) {
        arity!(args, 0);
        set_dir(recv, true)
    }
    def "decrypt" (recv, args, _block) {
        arity!(args, 0);
        set_dir(recv, false)
    }
    def "key=" (recv, args, _block) {
        arity!(args, 1);
        set_param(recv, &args[0], true)
    }
    def "iv=" (recv, args, _block) {
        arity!(args, 1);
        set_param(recv, &args[0], false)
    }
    // AEAD nonce-length override; must precede `iv=`.
    def "iv_len=" (recv, args, _block) {
        arity!(args, 1);
        let n = convert::to_index(&args[0])?;
        let c = cipher_of(recv);
        let mut st = c.st.lock();
        if !st.aead {
            return Err(cipher_error("cipher does not support AEAD".to_string()));
        }
        st.iv_len = Some(n as usize);
        if st.ctx.is_some() {
            rebuild(&mut st)?;
        }
        Ok(args[0].clone())
    }
    // `random_key`/`random_iv` -- upstream's `cipher.rb`: draw CSPRNG bytes
    // of the right length, assign, answer them.
    def "random_key" (recv, args, _block) {
        arity!(args, 0);
        let want = {
            let st = cipher_of(recv).st.lock();
            fetch(&st.fetch_name)?.key_length()
        };
        let mut buf = vec![0u8; want];
        fill_random(&mut buf)?;
        set_param(recv, &bin_str(buf), true)
    }
    def "random_iv" (recv, args, _block) {
        arity!(args, 0);
        let want = {
            let st = cipher_of(recv).st.lock();
            let n = st.iv_len;
            n.unwrap_or(fetch(&st.fetch_name)?.iv_length())
        };
        let mut buf = vec![0u8; want];
        fill_random(&mut buf)?;
        set_param(recv, &bin_str(buf), false)
    }
    // `pkcs5_keyivgen(pass, salt = nil, iterations = 2048, digest = "MD5")`
    // -- EVP_BytesToKey, the legacy PBE derivation.
    def "pkcs5_keyivgen" arity -1 (recv, args, _block) {
        arity!(args, 1..=4);
        let pass = str_bytes(&args[0])?;
        let salt = match args.get(1) {
            None | Some(RubyValue::Nil) => None,
            Some(v) => {
                let s = str_bytes(v)?;
                if s.len() != 8 {
                    return Err(arg_error!("salt must be an 8-octet string"));
                }
                Some(s)
            }
        };
        let iterations = match args.get(2) {
            None | Some(RubyValue::Nil) => 2048,
            Some(v) => convert::to_index(v)? as i32,
        };
        let (md, _) = match args.get(3) {
            None | Some(RubyValue::Nil) => super::md_by_name("MD5")?,
            Some(v) => md_from_value(v)?,
        };
        let c = cipher_of(recv);
        let mut st = c.st.lock();
        let cipher = fetch(&st.fetch_name)?;
        let legacy = openssl::symm::Cipher::from_nid(cipher.nid())
            .ok_or_else(|| cipher_error("unsupported cipher for keyivgen".to_string()))?;
        let derived = openssl::pkcs5::bytes_to_key(legacy, md, &pass, salt.as_deref(), iterations)
            .map_err(|e| cipher_error(stack_reason(&e)))?;
        st.key = Some(derived.key);
        st.iv = derived.iv;
        if st.ctx.is_some() {
            rebuild(&mut st)?;
        }
        Ok(RubyValue::Nil)
    }

    def "update" (recv, args, _block) {
        arity!(args, 1);
        let data = str_bytes(&args[0])?;
        let c = cipher_of(recv);
        let mut st = c.st.lock();
        let ctx = ready(&mut st)?;
        let mut out = Vec::new();
        ctx.cipher_update_vec(&data, &mut out)
            .map_err(|e| cipher_error(format!("cipher update failed: {}", stack_reason(&e))))?;
        Ok(bin_str(out))
    }
    // AAD for an AEAD mode -- an update with no output.
    def "auth_data=" (recv, args, _block) {
        arity!(args, 1);
        let data = str_bytes(&args[0])?;
        let c = cipher_of(recv);
        let mut st = c.st.lock();
        if !st.aead {
            return Err(cipher_error("cipher does not support AEAD".to_string()));
        }
        let ctx = ready(&mut st)?;
        ctx.cipher_update(&data, None)
            .map_err(|e| cipher_error(format!("cipher update failed: {}", stack_reason(&e))))?;
        Ok(args[0].clone())
    }
    def "final" (recv, args, _block) {
        arity!(args, 0);
        let c = cipher_of(recv);
        let mut st = c.st.lock();
        let aead_decrypt = st.aead && st.dir == Some(false);
        let ctx = ready(&mut st)?;
        let mut out = Vec::new();
        match ctx.cipher_final_vec(&mut out) {
            Ok(_) => Ok(bin_str(out)),
            Err(_) if aead_decrypt => Err(raise_error(
                "OpenSSL::Cipher::AuthTagError",
                "AEAD authentication tag verification failed".to_string(),
            )),
            Err(e) => Err(cipher_error(format!(
                "cipher final failed: {}",
                stack_reason(&e)
            ))),
        }
    }
    // The tag an AEAD encryption produced (after `final`), 16 bytes unless
    // narrowed.
    def "auth_tag" arity -1 (recv, args, _block) {
        arity!(args, 0..=1);
        let len = match args.first() {
            None => 16,
            Some(v) => convert::to_index(v)? as usize,
        };
        let c = cipher_of(recv);
        let mut st = c.st.lock();
        if !st.aead {
            return Err(cipher_error("authentication tag not supported by this cipher".to_string()));
        }
        let ctx = ready(&mut st)?;
        let mut tag = vec![0u8; len];
        ctx.tag(&mut tag)
            .map_err(|e| cipher_error(format!("retrieving the authentication tag failed: {}", stack_reason(&e))))?;
        Ok(bin_str(tag))
    }
    // The tag an AEAD decryption must verify against (before `final`).
    def "auth_tag=" (recv, args, _block) {
        arity!(args, 1);
        let tag = str_bytes(&args[0])?;
        let c = cipher_of(recv);
        let mut st = c.st.lock();
        if !st.aead {
            return Err(cipher_error("authentication tag not supported by this cipher".to_string()));
        }
        let ctx = ready(&mut st)?;
        ctx.set_tag(&tag)
            .map_err(|e| cipher_error(format!("setting the authentication tag failed: {}", stack_reason(&e))))?;
        Ok(args[0].clone())
    }
    def "padding=" (recv, args, _block) {
        arity!(args, 1);
        let pad = convert::to_index(&args[0])? != 0;
        let c = cipher_of(recv);
        let mut st = c.st.lock();
        st.padding = pad;
        if let Some(ctx) = st.ctx.as_mut() {
            ctx.set_padding(pad);
        }
        Ok(args[0].clone())
    }
    // Restart from the stored parameters -- EVP_CipherInit's re-init, so
    // direction, key and the ORIGINAL iv survive; fed data does not.
    def "reset" (recv, args, _block) {
        arity!(args, 0);
        let c = cipher_of(recv);
        let mut st = c.st.lock();
        if st.ctx.is_some() {
            rebuild(&mut st)?;
        }
        Ok(recv.clone())
    }

    def "name" (recv, args, _block) {
        arity!(args, 0);
        Ok(str(cipher_of(recv).st.lock().name.clone()))
    }
    def "key_len" (recv, args, _block) {
        arity!(args, 0);
        let st = cipher_of(recv).st.lock();
        Ok(RubyValue::Int(fetch(&st.fetch_name)?.key_length() as i64))
    }
    def "iv_len" (recv, args, _block) {
        arity!(args, 0);
        let st = cipher_of(recv).st.lock();
        let n = st.iv_len.unwrap_or(fetch(&st.fetch_name)?.iv_length());
        Ok(RubyValue::Int(n as i64))
    }
    def "block_size" (recv, args, _block) {
        arity!(args, 0);
        let st = cipher_of(recv).st.lock();
        Ok(RubyValue::Int(fetch(&st.fetch_name)?.block_size() as i64))
    }
    def "authenticated?" (recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(cipher_of(recv).st.lock().aead))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtins::registered_table;

    fn s(text: &str) -> RubyValue {
        RubyValue::Str(crate::string_new(text.to_string()))
    }
    fn bytes(text: &[u8]) -> RubyValue {
        RubyValue::Str(crate::string_from_bytes(text.to_vec(), crate::encoding::ASCII_8BIT))
    }
    fn im(name: &str) -> crate::builtins::BuiltinMethodFn {
        let table = registered_table(zeo_abi::OPENSSL_CIPHER_CLASS)
            .and_then(|t| t.instance.as_ref())
            .expect("Cipher registers an instance table");
        (table.lookup)(name).expect("instance method exists")
    }
    fn cm(name: &str) -> crate::builtins::BuiltinMethodFn {
        let table = registered_table(zeo_abi::OPENSSL_CIPHER_CLASS)
            .and_then(|t| t.class.as_ref())
            .expect("Cipher registers a class table");
        (table.lookup)(name).expect("class method exists")
    }
    fn hex_of(v: Result<RubyValue, Signal>) -> String {
        match v.unwrap() {
            RubyValue::Str(s) => super::super::hex(s.lock().bytes()),
            other => panic!("expected Str, got {other:?}"),
        }
    }
    fn raw(v: Result<RubyValue, Signal>) -> Vec<u8> {
        match v.unwrap() {
            RubyValue::Str(s) => s.lock().bytes().to_vec(),
            other => panic!("expected Str, got {other:?}"),
        }
    }

    #[test]
    fn cbc_round_trip_matches_ruby_bytes() {
        // ruby 4.0.6, key "\x01"*32, iv "\x02"*16, "secret message!!".
        let c = cm("new")(&RubyValue::Nil, &[s("AES-256-CBC")], None).unwrap();
        im("encrypt")(&c, &[], None).unwrap();
        im("key=")(&c, &[bytes(&[1u8; 32])], None).unwrap();
        im("iv=")(&c, &[bytes(&[2u8; 16])], None).unwrap();
        let mut ct = raw(im("update")(&c, &[s("secret message!!")], None));
        ct.extend(raw(im("final")(&c, &[], None)));
        assert_eq!(
            super::super::hex(&ct),
            "dd872cd9b22f5fa7870b7956830f7fd54ca58dc33e4a670c35a8d4283b614b61"
        );
        let d = cm("new")(&RubyValue::Nil, &[s("aes-256-cbc")], None).unwrap();
        im("decrypt")(&d, &[], None).unwrap();
        im("key=")(&d, &[bytes(&[1u8; 32])], None).unwrap();
        im("iv=")(&d, &[bytes(&[2u8; 16])], None).unwrap();
        let mut pt = raw(im("update")(&d, &[bytes(&ct)], None));
        pt.extend(raw(im("final")(&d, &[], None)));
        assert_eq!(pt, b"secret message!!");
    }

    #[test]
    fn ctr_stream_matches_ruby_bytes() {
        let c = cm("new")(&RubyValue::Nil, &[s("AES-128-CTR")], None).unwrap();
        im("encrypt")(&c, &[], None).unwrap();
        im("key=")(&c, &[bytes(&[3u8; 16])], None).unwrap();
        im("iv=")(&c, &[bytes(&[4u8; 16])], None).unwrap();
        let mut ct = raw(im("update")(&c, &[s("stream me")], None));
        ct.extend(raw(im("final")(&c, &[], None)));
        assert_eq!(super::super::hex(&ct), "9d19f8e0ce7d836aca");
    }

    #[test]
    fn gcm_tags_and_verifies_like_ruby() {
        let key = bytes(&[1u8; 32]);
        let c = cm("new")(&RubyValue::Nil, &[s("aes-256-gcm")], None).unwrap();
        im("encrypt")(&c, &[], None).unwrap();
        im("key=")(&c, &[key.clone()], None).unwrap();
        im("iv_len=")(&c, &[RubyValue::Int(12)], None).unwrap();
        im("iv=")(&c, &[bytes(&[5u8; 12])], None).unwrap();
        im("auth_data=")(&c, &[s("aad")], None).unwrap();
        let mut ct = raw(im("update")(&c, &[s("authenticated!")], None));
        ct.extend(raw(im("final")(&c, &[], None)));
        assert_eq!(super::super::hex(&ct), "60d7955d07dc47ee167b0cf9a5b2");
        let tag = raw(im("auth_tag")(&c, &[], None));
        assert_eq!(super::super::hex(&tag), "39d27b49f0fe0083133a2c2aa919ffd8");

        let d = cm("new")(&RubyValue::Nil, &[s("aes-256-gcm")], None).unwrap();
        im("decrypt")(&d, &[], None).unwrap();
        im("key=")(&d, &[key], None).unwrap();
        im("iv=")(&d, &[bytes(&[5u8; 12])], None).unwrap();
        im("auth_tag=")(&d, &[bytes(&tag)], None).unwrap();
        im("auth_data=")(&d, &[s("aad")], None).unwrap();
        let mut pt = raw(im("update")(&d, &[bytes(&ct)], None));
        pt.extend(raw(im("final")(&d, &[], None)));
        assert_eq!(pt, b"authenticated!");
    }

    #[test]
    fn chacha20_poly1305_matches_ruby_bytes() {
        let c = cm("new")(&RubyValue::Nil, &[s("chacha20-poly1305")], None).unwrap();
        im("encrypt")(&c, &[], None).unwrap();
        im("key=")(&c, &[bytes(&[1u8; 32])], None).unwrap();
        im("iv=")(&c, &[bytes(&[6u8; 12])], None).unwrap();
        im("auth_data=")(&c, &[s("")], None).unwrap();
        let mut ct = raw(im("update")(&c, &[s("chacha!")], None));
        ct.extend(raw(im("final")(&c, &[], None)));
        assert_eq!(super::super::hex(&ct), "5072c3b04c904c");
        assert_eq!(
            hex_of(im("auth_tag")(&c, &[], None)),
            "b5b0bf561a5de6d1a3b286aa1e794116"
        );
    }

    #[test]
    fn no_padding_matches_ruby_bytes() {
        let c = cm("new")(&RubyValue::Nil, &[s("AES-256-CBC")], None).unwrap();
        im("encrypt")(&c, &[], None).unwrap();
        im("key=")(&c, &[bytes(&[1u8; 32])], None).unwrap();
        im("iv=")(&c, &[bytes(&[2u8; 16])], None).unwrap();
        im("padding=")(&c, &[RubyValue::Int(0)], None).unwrap();
        let mut ct = raw(im("update")(&c, &[s("0123456789abcdef")], None));
        ct.extend(raw(im("final")(&c, &[], None)));
        assert_eq!(
            super::super::hex(&ct),
            "2e87d243c361cf658497b59d01f0aa40"
        );
    }

    #[test]
    fn reset_replays_the_original_iv() {
        let c = cm("new")(&RubyValue::Nil, &[s("AES-256-CBC")], None).unwrap();
        im("encrypt")(&c, &[], None).unwrap();
        im("key=")(&c, &[bytes(&[1u8; 32])], None).unwrap();
        im("iv=")(&c, &[bytes(&[2u8; 16])], None).unwrap();
        let mut a = raw(im("update")(&c, &[s("hello")], None));
        a.extend(raw(im("final")(&c, &[], None)));
        im("reset")(&c, &[], None).unwrap();
        let mut b = raw(im("update")(&c, &[s("hello")], None));
        b.extend(raw(im("final")(&c, &[], None)));
        assert_eq!(a, b);
    }

    #[test]
    fn metadata_matches_ruby() {
        let c = cm("new")(&RubyValue::Nil, &[s("aes-256-cbc")], None).unwrap();
        let name = im("name")(&c, &[], None).unwrap();
        let RubyValue::Str(name) = name else { panic!("expected Str") };
        assert_eq!(name.lock().to_utf8_lossy(), "AES-256-CBC");
        assert!(matches!(im("key_len")(&c, &[], None).unwrap(), RubyValue::Int(32)));
        assert!(matches!(im("iv_len")(&c, &[], None).unwrap(), RubyValue::Int(16)));
        assert!(matches!(im("block_size")(&c, &[], None).unwrap(), RubyValue::Int(16)));
        assert!(matches!(im("authenticated?")(&c, &[], None).unwrap(), RubyValue::Bool(false)));
        let g = cm("new")(&RubyValue::Nil, &[s("aes-256-gcm")], None).unwrap();
        assert!(matches!(im("authenticated?")(&g, &[], None).unwrap(), RubyValue::Bool(true)));
    }

    #[test]
    fn keyivgen_derives_deterministically() {
        let c = cm("new")(&RubyValue::Nil, &[s("aes-256-cbc")], None).unwrap();
        im("encrypt")(&c, &[], None).unwrap();
        im("pkcs5_keyivgen")(&c, &[s("passphrase")], None).unwrap();
        let mut a = raw(im("update")(&c, &[s("hello")], None));
        a.extend(raw(im("final")(&c, &[], None)));
        let d = cm("new")(&RubyValue::Nil, &[s("aes-256-cbc")], None).unwrap();
        im("decrypt")(&d, &[], None).unwrap();
        im("pkcs5_keyivgen")(&d, &[s("passphrase")], None).unwrap();
        let mut pt = raw(im("update")(&d, &[bytes(&a)], None));
        pt.extend(raw(im("final")(&d, &[], None)));
        assert_eq!(pt, b"hello");
    }
}
