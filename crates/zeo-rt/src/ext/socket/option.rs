//! `Socket::Option` -- one socket option as a value: its address family, its
//! protocol level, its option name, and the raw bytes the kernel stores. This
//! is what `BasicSocket#getsockopt` answers and what `#setsockopt` accepts in
//! place of a `(level, optname, value)` triple.
//!
//! Semantics oracle-verified against ruby 4.0.6: `#int` reads a native `int`
//! out of the data (a wrong-sized option is a `TypeError`), `#bool` is that int
//! made truthy, `#level`/`#optname` answer the platform numbers, and `#inspect`
//! names the family, level, and option (`"#<Socket::Option: INET SOCKET
//! KEEPALIVE 1>"`), falling back to `level:N optname:N "bytes"` for a pair it
//! cannot name.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::binary_string;
use crate::builtins::{arity, type_error};
use crate::dispatch::{RObj, RubyObject};
use crate::{ClassId, RubyValue, Signal, string_new};
use zeo_abi::SOCKET_OPTION_CLASS;
use zeo_macros::ruby_class;

pub struct RSockOpt {
    family: i32,
    level: i32,
    optname: i32,
    data: Vec<u8>,
    frozen: AtomicBool,
}

impl RSockOpt {
    fn new(family: i32, level: i32, optname: i32, data: Vec<u8>) -> Arc<RSockOpt> {
        Arc::new(RSockOpt {
            family,
            level,
            optname,
            data,
            frozen: AtomicBool::new(false),
        })
    }
}

impl RubyObject for RSockOpt {
    fn class_id(&self) -> ClassId {
        SOCKET_OPTION_CLASS
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
        let o = RSockOpt::new(self.family, self.level, self.optname, self.data.clone());
        if copy_frozen && self.is_frozen() {
            o.set_frozen();
        }
        o
    }
}

/// A ready-made value from a `getsockopt(2)` answer.
pub(crate) fn from_raw(family: i32, level: i32, optname: i32, data: Vec<u8>) -> RubyValue {
    RubyValue::Object(RSockOpt::new(family, level, optname, data))
}

/// The `(level, optname, data)` of a value that IS a `Socket::Option`, which is
/// what `#setsockopt`'s one-argument form takes; `None` for anything else.
pub(crate) fn parts(v: &RubyValue) -> Option<(i32, i32, Vec<u8>)> {
    let RubyValue::Object(o) = v else {
        return None;
    };
    let o = o.as_any().downcast_ref::<RSockOpt>()?;
    Some((o.level, o.optname, o.data.clone()))
}

fn opt_of(recv: &RubyValue) -> &RSockOpt {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RSockOpt>()
            .expect("the Socket::Option table only dispatches on Socket::Option receivers"),
        _ => unreachable!("the Socket::Option table only dispatches on Socket::Option receivers"),
    }
}

/// The address families `#inspect` names, and that a Symbol argument resolves
/// against.
const FAMILIES: &[(i32, &str)] = &[
    (libc::AF_UNSPEC, "UNSPEC"),
    (libc::AF_INET, "INET"),
    (libc::AF_INET6, "INET6"),
    (libc::AF_UNIX, "UNIX"),
];

/// The protocol levels, read the same two ways.
const LEVELS: &[(i32, &str)] = &[
    (libc::SOL_SOCKET, "SOCKET"),
    (libc::IPPROTO_IP, "IP"),
    (libc::IPPROTO_IPV6, "IPV6"),
    (libc::IPPROTO_TCP, "TCP"),
    (libc::IPPROTO_UDP, "UDP"),
];

/// The `(level, optname, name)` triples, covering exactly the option constants
/// zeo seeds on `Socket` -- so a name this can print is a name a program can
/// also spell.
const OPTNAMES: &[(i32, i32, &str)] = &[
    (libc::SOL_SOCKET, libc::SO_REUSEADDR, "REUSEADDR"),
    (libc::SOL_SOCKET, libc::SO_REUSEPORT, "REUSEPORT"),
    (libc::SOL_SOCKET, libc::SO_KEEPALIVE, "KEEPALIVE"),
    (libc::SOL_SOCKET, libc::SO_BROADCAST, "BROADCAST"),
    (libc::SOL_SOCKET, libc::SO_LINGER, "LINGER"),
    (libc::SOL_SOCKET, libc::SO_SNDBUF, "SNDBUF"),
    (libc::SOL_SOCKET, libc::SO_RCVBUF, "RCVBUF"),
    (libc::SOL_SOCKET, libc::SO_ERROR, "ERROR"),
    (libc::SOL_SOCKET, libc::SO_TYPE, "TYPE"),
    (libc::SOL_SOCKET, libc::SO_DONTROUTE, "DONTROUTE"),
    (libc::SOL_SOCKET, libc::SO_OOBINLINE, "OOBINLINE"),
    (libc::IPPROTO_TCP, libc::TCP_NODELAY, "NODELAY"),
    (libc::IPPROTO_IP, libc::IP_TTL, "TTL"),
    (libc::IPPROTO_IP, libc::IP_MULTICAST_TTL, "MULTICAST_TTL"),
    (libc::IPPROTO_IP, libc::IP_MULTICAST_LOOP, "MULTICAST_LOOP"),
    (libc::IPPROTO_IP, libc::IP_ADD_MEMBERSHIP, "ADD_MEMBERSHIP"),
    (
        libc::IPPROTO_IP,
        libc::IP_DROP_MEMBERSHIP,
        "DROP_MEMBERSHIP",
    ),
    (libc::IPPROTO_IPV6, libc::IPV6_V6ONLY, "V6ONLY"),
    (
        libc::IPPROTO_IPV6,
        libc::IPV6_MULTICAST_HOPS,
        "MULTICAST_HOPS",
    ),
    (libc::IPPROTO_IPV6, libc::IPV6_UNICAST_HOPS, "UNICAST_HOPS"),
];

/// A family/level/optname argument: an Integer, or the bare Symbol/String CRuby
/// also accepts (`:INET`, `:SOCKET`, `:KEEPALIVE`). An optname resolves within
/// the `level` already parsed, since the same number means different options at
/// different levels.
pub(crate) fn opt_int(v: &RubyValue, level: Option<i32>) -> Result<i32, Signal> {
    let name = match v {
        RubyValue::Int(n) => return Ok(*n as i32),
        RubyValue::Symbol(s) => s.name(),
        RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
        other => return Ok(crate::builtins::convert::to_index(other)? as i32),
    };
    let bare = name
        .trim_start_matches("AF_")
        .trim_start_matches("PF_")
        .trim_start_matches("SOL_")
        .trim_start_matches("SO_")
        .trim_start_matches("IPPROTO_");
    if let Some(lvl) = level {
        if let Some((_, opt, _)) = OPTNAMES.iter().find(|(l, _, n)| *l == lvl && *n == bare) {
            return Ok(*opt);
        }
    } else if let Some((f, _)) = FAMILIES.iter().find(|(_, n)| *n == bare) {
        return Ok(*f);
    } else if let Some((l, _)) = LEVELS.iter().find(|(_, n)| *n == bare) {
        return Ok(*l);
    }
    Err(type_error!("unknown socket option name: {name}"))
}

/// The native `int` an option's data holds. CRuby insists on the exact width,
/// so a linger (8 bytes) read as an int is a `TypeError`, not a truncation.
fn data_int(o: &RSockOpt) -> Result<i32, Signal> {
    let want = std::mem::size_of::<libc::c_int>();
    if o.data.len() != want {
        return Err(type_error!(
            "size differ.  expected as sizeof(int)={want} but {}",
            o.data.len()
        ));
    }
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(&o.data[..4]);
    Ok(i32::from_ne_bytes(bytes))
}

fn int_bytes(n: i32) -> Vec<u8> {
    (n as libc::c_int).to_ne_bytes().to_vec()
}

/// The `struct linger` an SO_LINGER option carries: `(l_onoff, l_linger)`.
fn linger_pair(o: &RSockOpt) -> Option<(i32, i32)> {
    if o.data.len() < 8 {
        return None;
    }
    let read = |i: usize| {
        let mut b = [0u8; 4];
        b.copy_from_slice(&o.data[i..i + 4]);
        i32::from_ne_bytes(b)
    };
    Some((read(0), read(4)))
}

fn name_of(table: &[(i32, &str)], v: i32) -> Option<String> {
    table
        .iter()
        .find(|(n, _)| *n == v)
        .map(|(_, name)| (*name).to_string())
}

/// The value half of `#inspect`: the linger pair, the socktype name for
/// SO_TYPE, else the plain int, else the raw bytes inspected.
fn inspect_value(o: &RSockOpt) -> String {
    if o.level == libc::SOL_SOCKET && o.optname == libc::SO_LINGER {
        if let Some((onoff, secs)) = linger_pair(o) {
            let state = if onoff != 0 { "on" } else { "off" };
            return format!("{state} {secs}sec");
        }
    }
    match data_int(o) {
        Ok(n) if o.level == libc::SOL_SOCKET && o.optname == libc::SO_TYPE => match n {
            libc::SOCK_STREAM => "SOCK_STREAM".to_string(),
            libc::SOCK_DGRAM => "SOCK_DGRAM".to_string(),
            libc::SOCK_RAW => "SOCK_RAW".to_string(),
            other => other.to_string(),
        },
        Ok(n) => n.to_string(),
        Err(_) => binary_string(o.data.clone()).inspect_string(),
    }
}

fn str_val(s: impl Into<String>) -> RubyValue {
    RubyValue::Str(string_new(s.into()))
}

ruby_class! {
    Option = zeo_abi::SOCKET_OPTION_CLASS < zeo_abi::OBJECT_CLASS;

    // `Socket::Option.new(family, level, optname, data)` -- the raw form.
    def self."new"(_recv, args, _block) {
        arity!(args, 4);
        let family = opt_int(&args[0], None)?;
        let level = opt_int(&args[1], None)?;
        let optname = opt_int(&args[2], Some(level))?;
        let data = crate::builtins::convert::to_rstr(&args[3])?.lock().bytes().to_vec();
        Ok(RubyValue::Object(RSockOpt::new(family, level, optname, data)))
    }
    // `Socket::Option.int(family, level, optname, integer)`.
    def self."int"(_recv, args, _block) {
        arity!(args, 4);
        let family = opt_int(&args[0], None)?;
        let level = opt_int(&args[1], None)?;
        let optname = opt_int(&args[2], Some(level))?;
        let n = crate::builtins::convert::to_index(&args[3])? as i32;
        Ok(RubyValue::Object(RSockOpt::new(family, level, optname, int_bytes(n))))
    }
    // `Socket::Option.bool(family, level, optname, flag)` -- an int option
    // holding 0 or 1.
    def self."bool"(_recv, args, _block) {
        arity!(args, 4);
        let family = opt_int(&args[0], None)?;
        let level = opt_int(&args[1], None)?;
        let optname = opt_int(&args[2], Some(level))?;
        let n = i32::from(args[3].truthy());
        Ok(RubyValue::Object(RSockOpt::new(family, level, optname, int_bytes(n))))
    }
    // `Socket::Option.linger(onoff, secs)` -- the SO_LINGER pair. Its family is
    // AF_UNSPEC: linger is not family-specific.
    def self."linger"(_recv, args, _block) {
        arity!(args, 2);
        let onoff = match &args[0] {
            RubyValue::Int(n) => *n as i32,
            v => i32::from(v.truthy()),
        };
        let secs = crate::builtins::convert::to_index(&args[1])? as i32;
        let mut data = int_bytes(onoff);
        data.extend_from_slice(&int_bytes(secs));
        Ok(RubyValue::Object(RSockOpt::new(
            libc::AF_UNSPEC,
            libc::SOL_SOCKET,
            libc::SO_LINGER,
            data,
        )))
    }

    def "family"(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(opt_of(recv).family as i64))
    }
    def "level"(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(opt_of(recv).level as i64))
    }
    def "optname"(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(opt_of(recv).optname as i64))
    }
    // `#data`/`#to_s` -- the kernel's own bytes (ASCII-8BIT).
    def "data" | "to_s"(recv, args, _block) {
        arity!(args, 0);
        Ok(binary_string(opt_of(recv).data.clone()))
    }
    def "int"(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(data_int(opt_of(recv))? as i64))
    }
    def "bool"(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(data_int(opt_of(recv))? != 0))
    }
    // `#linger` -- `[onoff, secs]` for an SO_LINGER option.
    def "linger"(recv, args, _block) {
        arity!(args, 0);
        let o = opt_of(recv);
        let Some((onoff, secs)) = linger_pair(o) else {
            return Err(type_error!("size differ.  expected as sizeof(struct linger)=8 but {}", o.data.len()));
        };
        Ok(RubyValue::Array(crate::array_new(vec![
            RubyValue::Bool(onoff != 0),
            RubyValue::Int(secs as i64),
        ])))
    }
    def "inspect"(recv, args, _block) {
        arity!(args, 0);
        let o = opt_of(recv);
        let family = name_of(FAMILIES, o.family).unwrap_or_else(|| o.family.to_string());
        let optname = OPTNAMES
            .iter()
            .find(|(l, n, _)| *l == o.level && *n == o.optname)
            .map(|(_, _, name)| (*name).to_string());
        // A level or option zeo cannot name prints as CRuby's numeric fallback,
        // which spells the data rather than guessing at its shape.
        let body = match (name_of(LEVELS, o.level), optname) {
            (Some(level), Some(optname)) => {
                format!("{family} {level} {optname} {}", inspect_value(o))
            }
            _ => format!(
                "{family} level:{} optname:{} {}",
                o.level,
                o.optname,
                binary_string(o.data.clone()).inspect_string()
            ),
        };
        Ok(str_val(format!("#<Socket::Option: {body}>")))
    }
}
