//! The compiler/runtime ABI, single-sourced.
//!
//! `zeo` (the compiler) and `zeo-rt` (the runtime every generated
//! program links) deliberately never link each other -- but they must agree
//! on the numeric identity of every built-in class: the compiler bakes
//! `ClassId`s into generated code as literals, and the runtime's dispatch/
//! `is_a?`/registry machinery interprets them. This crate is the one source
//! of truth both sides re-export, so the agreement can't drift as the ABI
//! gains class names, module-ness, and per-box method-table keys.
//!
//! The table also carries each builtin's SUPERCLASS and INCLUDES -- the
//! CRuby-exact hierarchy (oracle-verified against ruby 4.0.6) that both the
//! compiler's ancestor linearization and the runtime's registry-free fallback
//! chains are derived from. Ids are APPEND-ONLY: renumbering is technically
//! safe (nothing persists across builds), but appending keeps generated-code
//! diffs reviewable and eliminates any stale-incremental-artifact risk.
//!
//! Zero dependencies, on purpose: keeping the runtime's heavy deps out of
//! every compiler build -- a dependency-free leaf has no such cost.

pub mod abi;
mod builtins;
mod errno;
pub mod ffi;

pub use builtins::{
    BUILTINS, INSTANCELESS, NOT_PAYLOAD_ROOTS, builtin_class, is_instanceless, is_payload_root,
    payload_root_of,
};
pub use errno::{ERRNO_ALIASES, ERRNO_CLASSES, ErrnoClass};

mod exceptions;
mod features;
mod hierarchy;
mod ids;
// A glob here, because these modules are this root's own tables split
// for size: every name they hold is `zeo_abi::NAME` to a consumer, and
// an explicit list of 180 ids would be a second copy of `ids.rs`.
pub use exceptions::*;
pub use features::*;
pub use hierarchy::*;
pub use ids::*;

/// Identifies a Ruby class at runtime AND at compile time -- the compiler
/// mirrors of this id are baked into generated code as literals, so the two
/// sides genuinely share one numbering (this type), not two synced copies.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ClassId(pub u32);

/// The first id handed out to a class created at RUNTIME (`Class.new`, and any
/// future eval-defined class -- see the runtime overlay in
/// `zeo-rt/src/runtime_meta.rs`). Compile-time ids are dense and small:
/// builtins occupy `0..~60` and user classes count up from there by program
/// size, so a `1 << 30` base leaves the entire low range to the AOT compiler
/// while staying trivially distinguishable at runtime (`id.0 >= this` answers
/// "was this class born at runtime?", which selects the overlay's ancestor-
/// walking resolution instead of the frozen flat-table fast path).
pub const RUNTIME_CLASS_ID_BASE: u32 = 1 << 30;

/// The id `ENV`'s method table is filed under -- a table key, NOT a class.
///
/// `ENV.class` is `Object` (oracle-verified: ENV is a lone singleton carrying
/// Hash-shaped methods, not a Hash instance), so its rows cannot be keyed by
/// its real class -- they would answer for every plain Object in the program.
/// Dispatch reaches them by IDENTITY instead (`builtins::env::is_env_obj`), and
/// this id exists only so the rows can live in the same `ruby_class!` DSL as
/// every other builtin rather than in a hand-rolled lookup. Nothing ever
/// REPORTS this id as its class, and it sits past both the compiler's dense low
/// ids and the runtime block above, so it collides with nothing.
pub const ENV_SINGLETON_CLASS: ClassId = ClassId(u32::MAX);

/// The Ruby language/library level zeo targets, single-sourced here so the
/// runtime's `RUBY_VERSION`/`RUBY_ENGINE_VERSION` seeding (`zeo-rt`'s
/// `bootstrap`) and the compiler's compile-time version-gate folding
/// (`zeo`'s `version_fold`) agree byte-for-byte -- a `RUBY_VERSION < "x"`
/// guard must fold against the SAME string the running program reports.
pub const RUBY_VERSION: &str = "4.0.6";

/// The constants ruby seeds on `Object` before a program's first line runs.
///
/// Nothing in a program's own body assigns them, so a compile-time scan for
/// `NAME = ...` finds nothing and would answer `defined?(RUBY_ENGINE)` with
/// nil where ruby says `"constant"`. Single-sourced here because the two sides
/// of that answer live in different crates: `zeo-rt`'s `bootstrap` sets the
/// values, and the compiler needs only to know the names exist.
///
/// `test/compiler/guards/a_defined_guard_asks_about_this_moment.rb` reads every name on both
/// sides, so a name added here and not seeded (or seeded and not added) shows
/// up as a divergence from the oracle rather than as a quiet nil.
pub const SEEDED_OBJECT_CONSTANTS: &[&str] = &[
    "ARGF",
    "ARGV",
    // CRuby sets it at the VM level, `nil` for an ordinary build; mkmf
    // gates on it at module-body level. Missing from this list, every
    // `defined?(CROSS_COMPILING)` folds to nil.
    "CROSS_COMPILING",
    "ENV",
    "RUBY_COPYRIGHT",
    "RUBY_DESCRIPTION",
    "RUBY_ENGINE",
    "RUBY_ENGINE_VERSION",
    "RUBY_PATCHLEVEL",
    "RUBY_PLATFORM",
    "RUBY_RELEASE_DATE",
    "RUBY_REVISION",
    "RUBY_VERSION",
    "STDERR",
    "STDIN",
    "STDOUT",
];

/// The encoding a Regexp literal's trailing flag letter FORCES -- `/n`, `/e`,
/// `/s`, `/u`. `Source` is a plain literal, whose encoding follows its own
/// source bytes.
///
/// Shared between the compiler (which reads the letter off the literal) and the
/// runtime (which reports it), because it is observable three ways and the two
/// halves must not drift:
///
/// | | `#options` | `#encoding` | `#fixed_encoding?` |
/// |---|---|---|---|
/// | `Source` | `0` | its own bytes' | true only if non-ASCII |
/// | `/n` | `32` | `US-ASCII` (ASCII-only source) | `false` |
/// | `/e` | `16` | `EUC-JP` | `true` |
/// | `/s` | `16` | `Windows-31J` | `true` |
/// | `/u` | `16` | `UTF-8` | `true` |
/// | `Binary` | `0` | `US-ASCII` (ASCII-only source) | `false` |
///
/// A `/n` or `Binary` pattern holding a byte past 0x7f is pinned to
/// ASCII-8BIT instead, which adds `16` and makes it fixed.
///
/// ruby2ruby and ruby_parser both open by reading exactly those bits back out
/// of four throwaway literals (`ENC_EUC = /x/e.options`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RegexpEncoding {
    #[default]
    Source,
    /// `/n` -- ruby's ARG_ENCODING_NONE. Its pattern reads bytes.
    None,
    /// `Regexp.new` of an ASCII-8BIT String: the pattern reads bytes, as
    /// `/n` does, with no `n` flag of its own.
    Binary,
    /// `/e` -- EUC-JP.
    EucJp,
    /// `/s` -- Windows-31J.
    Windows31j,
    /// `/u` -- UTF-8.
    Utf8,
}

impl RegexpEncoding {
    /// The `Regexp::` bits this flag contributes to `#options`, above the
    /// `i`/`x`/`m` bits: `FIXEDENCODING` (16) for the three that pin a real
    /// encoding, `NOENCODING` (32) for `/n`.
    pub fn option_bits(self) -> i64 {
        match self {
            RegexpEncoding::Source | RegexpEncoding::Binary => 0,
            RegexpEncoding::None => 32,
            RegexpEncoding::EucJp | RegexpEncoding::Windows31j | RegexpEncoding::Utf8 => 16,
        }
    }

    /// Whether `#fixed_encoding?` is true on the flag alone. `/n` is not: it
    /// declares the pattern encoding-agnostic rather than pinning one.
    pub fn is_fixed(self) -> bool {
        // `Binary` is not either: only a high byte in the pattern pins it.
        matches!(
            self,
            RegexpEncoding::EucJp | RegexpEncoding::Windows31j | RegexpEncoding::Utf8
        )
    }
}

/// One reserved built-in class/module -- see [`BUILTINS`].
pub struct BuiltinClass {
    pub id: ClassId,
    /// The Ruby-visible name (`"Integer"`, `"Enumerable"`, ...).
    pub name: &'static str,
    /// `true` for a built-in MODULE (`Enumerable`, `Kernel`, `Math`): no
    /// superclass, never instantiated, participates in `ancestors` via
    /// `include` only.
    pub is_module: bool,
    /// CRuby's real superclass edge (oracle-verified). `None` for modules
    /// and for `BasicObject`, the true root.
    pub superclass: Option<ClassId>,
    /// CRuby's own mixins, in source order (oracle-verified): e.g.
    /// `Numeric` includes `Comparable`, `Array` includes `Enumerable`.
    pub includes: &'static [ClassId],
    /// The `require`-able feature that must be activated before this class's
    /// constant resolves -- CRuby's ext/ model, where `require "base64"`
    /// exposes `Base64`. `None` for always-on core classes (every current
    /// row except the in-tree `ext/` modules); `Some("base64")` for a
    /// require-gated extension. Referencing a gated class without its
    /// `require` is a `NameError`, exactly as in CRuby (see
    /// `zeo::Compiler::resolve_class`'s feature gate).
    pub feature: Option<&'static str>,
}

/// The constant names the native `socket` extension defines on `Socket`
/// (CRuby's `Socket::AF_INET6`, `Socket::SOCK_STREAM`, ...). SINGLE SOURCE OF
/// TRUTH shared by the two halves of the compiler: `zeo-rt`'s `seed_socket`
/// installs each name's host `libc` value at runtime (a `debug_assert` there
/// enforces that every name below maps to a value), and the compiler registers
/// each into `Socket`'s compile-time constant table so `Socket::X` reads,
/// `Socket.const_defined?(:X)`, `defined?(Socket::X)`, and the platform guards
/// gems write around them (`unless Socket.const_defined? :AF_INET6`) all resolve
/// with CRuby parity. Values live in `zeo-rt` (it owns the `libc` dependency);
/// only the names are needed at compile time, so this stays dependency-free.
pub const SOCKET_CONSTANT_NAMES: &[&str] = &[
    // Address / protocol families.
    "AF_UNSPEC",
    "AF_INET",
    "AF_INET6",
    "AF_UNIX",
    "AF_LOCAL",
    "PF_UNSPEC",
    "PF_INET",
    "PF_INET6",
    "PF_UNIX",
    "PF_LOCAL", // Socket types.
    "SOCK_STREAM",
    "SOCK_DGRAM",
    "SOCK_RAW",
    "SOCK_SEQPACKET",
    "SOCK_RDM",
    // IP protocols.
    "IPPROTO_IP",
    "IPPROTO_ICMP",
    "IPPROTO_TCP",
    "IPPROTO_UDP",
    "IPPROTO_IPV6",
    "IPPROTO_RAW",
    // Option levels and socket-level options.
    "SOL_SOCKET",
    "SO_REUSEADDR",
    "SO_REUSEPORT",
    "SO_KEEPALIVE",
    "SO_BROADCAST",
    "SO_LINGER",
    "SO_SNDBUF",
    "SO_RCVBUF",
    "SO_ERROR",
    "SO_TYPE",
    "SO_DONTROUTE",
    "SO_OOBINLINE",
    // TCP / IP / IPv6 options.
    "TCP_NODELAY",
    "IP_TTL",
    "IP_MULTICAST_TTL",
    "IP_MULTICAST_LOOP",
    "IP_ADD_MEMBERSHIP",
    "IP_DROP_MEMBERSHIP",
    "IPV6_V6ONLY",
    "IPV6_MULTICAST_HOPS",
    "IPV6_UNICAST_HOPS",
    // getaddrinfo / getnameinfo flags.
    "AI_PASSIVE",
    "AI_CANONNAME",
    "AI_NUMERICHOST",
    "AI_NUMERICSERV",
    "AI_ADDRCONFIG",
    "AI_V4MAPPED",
    "AI_ALL",
    "NI_NUMERICHOST",
    "NI_NUMERICSERV",
    "NI_NOFQDN",
    "NI_NAMEREQD",
    "NI_DGRAM", // Shutdown directions and message flags.
    "SHUT_RD",
    "SHUT_WR",
    "SHUT_RDWR",
    "MSG_OOB",
    "MSG_PEEK",
    "MSG_DONTROUTE",
    "MSG_WAITALL",
    "SO_RCVTIMEO",
    "SO_SNDTIMEO",
    "SO_RCVLOWAT",
    "SO_SNDLOWAT",
    "SO_DEBUG",
    "SO_ACCEPTCONN",
    "SOMAXCONN",
    "IP_TOS",
    "IP_HDRINCL",
    "IP_MULTICAST_IF",
    "IPV6_MULTICAST_IF",
    "IPV6_MULTICAST_LOOP",
    "IPV6_JOIN_GROUP",
    "IPV6_LEAVE_GROUP",
    "MSG_DONTWAIT",
    "MSG_TRUNC",
    "MSG_CTRUNC",
    "MSG_EOR",
    "IPPROTO_ICMPV6",
    "TCP_MAXSEG",
];

/// What a `Ruby::Box#eval` snippet calls its file, in EITHER tier: the
/// runtime `eval` entry names its frames this, and the compiler registers
/// a literal snippet under the same name when it splices one. A backtrace
/// must not say which tier ran the code.
pub const BOX_EVAL_FILE: &str = "eval";

#[cfg(test)]
mod tests {
    use super::*;

    /// The contiguity both consumers rely on: the compiler pushes
    /// `BUILTINS` in order into its class arena (so each entry's index must
    /// equal its id), and the runtime treats these ids as stable literals
    /// baked into generated programs.
    #[test]
    fn builtin_ids_are_contiguous_from_one() {
        for (i, b) in BUILTINS.iter().enumerate() {
            assert_eq!(b.id.0 as usize, i + 1, "{} out of order", b.name);
        }
    }

    /// `declared_ancestors` must match CRuby's own linearization (and thus the
    /// compiler's `mro::expand_into`, which reads the same edges), or the runtime
    /// registry and codegen would disagree on `is_a?`. Covers a builtin, a
    /// module, `Object`, and -- via the same DFS -- an exception chain.
    #[test]
    fn declared_ancestors_match_cruby() {
        let by_name = |n: &str| BUILTINS.iter().find(|b| b.name == n).unwrap().id;
        let exc = |n: &str| EXCEPTION_CLASSES.iter().find(|e| e.name == n).unwrap().id;
        let names = |ids: Vec<ClassId>| -> Vec<&'static str> {
            ids.into_iter()
                .map(|id| {
                    builtin_name(id).unwrap_or_else(|| {
                        EXCEPTION_CLASSES.iter().find(|e| e.id == id).unwrap().name
                    })
                })
                .collect()
        };
        assert_eq!(
            names(declared_ancestors(by_name("Integer"))),
            [
                "Integer",
                "Numeric",
                "Comparable",
                "Object",
                "Kernel",
                "BasicObject"
            ]
        );
        assert_eq!(
            names(declared_ancestors(by_name("Array"))),
            ["Array", "Enumerable", "Object", "Kernel", "BasicObject"]
        );
        assert_eq!(
            names(declared_ancestors(OBJECT_CLASS)),
            ["Object", "Kernel", "BasicObject"]
        );
        assert_eq!(
            names(declared_ancestors(by_name("Comparable"))),
            ["Comparable"]
        );
        // Exceptions linearize through the SAME DFS: superclass chain up to
        // `Exception`, then `Object`'s own tail. Oracle:
        // `StandardError.ancestors == [StandardError, Exception, Object, Kernel, BasicObject]`.
        assert_eq!(
            names(declared_ancestors(exc("StandardError"))),
            [
                "StandardError",
                "Exception",
                "Object",
                "Kernel",
                "BasicObject"
            ]
        );
        assert_eq!(
            names(declared_ancestors(exc("NoMethodError"))),
            [
                "NoMethodError",
                "NameError",
                "StandardError",
                "Exception",
                "Object",
                "Kernel",
                "BasicObject"
            ]
        );
        // The `Errno` namespace module: ancestors are just itself.
        assert_eq!(names(declared_ancestors(exc("Errno"))), ["Errno"]);
    }

    /// The exception classes start right after the last builtin and are
    /// contiguous, so `register_exceptions` (runtime) and the compiler's own
    /// sequential assignment land on the same id for each name.
    #[test]
    fn exception_ids_are_contiguous_after_the_builtins() {
        assert_eq!(
            FIRST_EXCEPTION_ID as usize,
            BUILTINS.len() + 1,
            "the exceptions must start right after the last builtin"
        );
        for (i, c) in EXCEPTION_CLASSES.iter().enumerate() {
            assert_eq!(
                c.id.0,
                FIRST_EXCEPTION_ID + i as u32,
                "{} out of order",
                c.name
            );
        }
    }

    /// Every named `*_CLASS` const points at the row of the same name -- the
    /// "keep the offset in sync with its row" promise, enforced.
    #[test]
    fn named_exception_consts_match_their_rows() {
        let name_of = |id: ClassId| EXCEPTION_CLASSES.iter().find(|e| e.id == id).unwrap().name;
        for (id, name) in [
            (EXCEPTION_CLASS, "Exception"),
            (NOT_IMPLEMENTED_ERROR_CLASS, "NotImplementedError"),
            (LOAD_ERROR_CLASS, "LoadError"),
            (ARGUMENT_ERROR_CLASS, "ArgumentError"),
            (
                UNDEFINED_CONVERSION_ERROR_CLASS,
                "Encoding::UndefinedConversionError",
            ),
            (
                INVALID_BYTE_SEQUENCE_ERROR_CLASS,
                "Encoding::InvalidByteSequenceError",
            ),
            (IO_ERROR_CLASS, "IOError"),
            (EOF_ERROR_CLASS, "EOFError"),
            (INDEX_ERROR_CLASS, "IndexError"),
            (KEY_ERROR_CLASS, "KeyError"),
            (STOP_ITERATION_CLASS, "StopIteration"),
            (NAME_ERROR_CLASS, "NameError"),
            (NO_METHOD_ERROR_CLASS, "NoMethodError"),
            (RANGE_ERROR_CLASS, "RangeError"),
            (FLOAT_DOMAIN_ERROR_CLASS, "FloatDomainError"),
            (LOCAL_JUMP_ERROR_CLASS, "LocalJumpError"),
            (REGEXP_ERROR_CLASS, "RegexpError"),
            (RUNTIME_ERROR_CLASS, "RuntimeError"),
            (FROZEN_ERROR_CLASS, "FrozenError"),
            (FIBER_ERROR_CLASS, "FiberError"),
            (THREAD_ERROR_CLASS, "ThreadError"),
            (TYPE_ERROR_CLASS, "TypeError"),
            (ZERO_DIVISION_ERROR_CLASS, "ZeroDivisionError"),
            (SYSTEM_CALL_ERROR_CLASS, "SystemCallError"),
            (ERRNO_MODULE, "Errno"),
            (SYNTAX_ERROR_CLASS, "SyntaxError"),
            (UNCAUGHT_THROW_ERROR_CLASS, "UncaughtThrowError"),
            (SYSTEM_EXIT_CLASS, "SystemExit"),
            (SIGNAL_EXCEPTION_CLASS, "SignalException"),
            (INTERRUPT_CLASS, "Interrupt"),
            (NO_MEMORY_ERROR_CLASS, "NoMemoryError"),
            (
                NO_MATCHING_PATTERN_KEY_ERROR_CLASS,
                "NoMatchingPatternKeyError",
            ),
            (RACTOR_REMOTE_ERROR_CLASS, "Ractor::RemoteError"),
            (FATAL_CLASS, "fatal"),
        ] {
            assert_eq!(name_of(id), name, "{name}'s const has drifted off its row");
        }
    }

    /// Every hierarchy edge points at a real table entry (or Object), and
    /// modules never claim a superclass -- the shape `Compiler::new`'s
    /// table walk and the runtime's registry-free chains both assume.
    #[test]
    fn hierarchy_edges_are_well_formed() {
        let exists = |id: ClassId| id == OBJECT_CLASS || builtin_name(id).is_some();
        for b in BUILTINS {
            if b.is_module {
                assert!(b.superclass.is_none(), "module {} has a superclass", b.name);
            }
            if let Some(sup) = b.superclass {
                assert!(exists(sup), "{}'s superclass id is unknown", b.name);
                assert_ne!(sup, b.id, "{} is its own superclass", b.name);
            }
            for &inc in b.includes {
                assert!(exists(inc), "{}'s include id is unknown", b.name);
            }
        }
        assert!(exists(OBJECT_SUPERCLASS));
        for &inc in OBJECT_INCLUDES {
            assert!(exists(inc));
        }
    }

    #[test]
    fn is_ext_feature_recognizes_only_gated_builtins() {
        assert!(is_ext_feature("base64"));
        assert!(is_ext_feature("stringio"));
        assert!(is_ext_feature("strscan"));
        assert!(is_ext_feature("digest"));
        assert!(is_ext_feature("json"));
        // Always-on core classes name no feature; core no-op requires
        // (`set`/`tmpdir`) aren't gated builtins either.
        assert!(!is_ext_feature("set"));
        assert!(!is_ext_feature("tmpdir"));
        assert!(!is_ext_feature("Integer"));
        assert!(!is_ext_feature("nonesuch"));
    }

    #[test]
    fn builtin_name_answers_object_builtins_and_unknowns() {
        assert_eq!(builtin_name(OBJECT_CLASS), Some("Object"));
        assert_eq!(builtin_name(INTEGER_CLASS), Some("Integer"));
        assert_eq!(builtin_name(STRUCT_CLASS), Some("Struct"));
        assert_eq!(builtin_name(ClassId(999)), None);
    }
}
