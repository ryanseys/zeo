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

mod errno;

pub use errno::{ERRNO_ALIASES, ERRNO_CLASSES, ErrnoClass};

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

/// Whether `name` is an in-tree `ext/` feature whose `require` activates a
/// gated builtin (`"base64"` -> `Base64`). The ABI table is the single
/// source of truth for the feature -> class mapping, so the compiler's
/// require loader and its constant resolver stay in lockstep automatically
/// as extensions are added. Distinct from always-on core no-op requires
/// (`"set"`, `"tmpdir"`), which name no gated class and are handled
/// separately by the loader.
pub fn is_ext_feature(name: &str) -> bool {
    BUILTINS.iter().any(|b| b.feature == Some(name))
}

/// Why a stdlib feature zeo DECLINES is missing -- appended to its
/// `LoadError` so a caller can tell a settled decision from a typo or an
/// unfinished feature. `None` for everything else, which keeps CRuby's bare
/// `cannot load such file -- <name>`.
///
/// Lives here because BOTH sides raise it and they never link each other: the
/// compiler's loader when it rejects the require outright, and the runtime's
/// `Kernel#require` when an unresolvable one was deferred. One source, so the
/// two wordings cannot drift.
///
/// See "Declined (a CRuby internal, not a missing binding)" in
/// `docs/COMPATIBILITY.md`.
pub fn declined_feature_reason(name: &str) -> Option<&'static str> {
    match name {
        "ripper" => Some(
            "declined. ripper exposes the reduction event stream of CRuby's parse.y, \
             and zeo's front end embeds prism -- a different parser with a different \
             event model, so there is nothing to bind. Use `require \"prism\"` for a \
             Ruby-level syntax tree. See docs/COMPATIBILITY.md.",
        ),
        _ => None,
    }
}

/// `ClassId(0)`, always present: the root every class ultimately chains up
/// to (via `BasicObject`). Not part of [`BUILTINS`] --
/// both sides construct/register `Object` specially (the compiler seeds it
/// as class index 0; the runtime's `Object` unit struct carries it as
/// `CLASS_ID`). Its own place in the chain is [`OBJECT_SUPERCLASS`] +
/// [`OBJECT_INCLUDES`]: `Object.ancestors == [Object, Kernel, BasicObject]`.
pub const OBJECT_CLASS: ClassId = ClassId(0);

pub const INTEGER_CLASS: ClassId = ClassId(1);
pub const FLOAT_CLASS: ClassId = ClassId(2);
pub const STRING_CLASS: ClassId = ClassId(3);
pub const SYMBOL_CLASS: ClassId = ClassId(4);
pub const ARRAY_CLASS: ClassId = ClassId(5);
pub const HASH_CLASS: ClassId = ClassId(6);
pub const RANGE_CLASS: ClassId = ClassId(7);
pub const NIL_CLASS: ClassId = ClassId(8);
pub const TRUE_CLASS: ClassId = ClassId(9);
pub const FALSE_CLASS: ClassId = ClassId(10);
pub const PROC_CLASS: ClassId = ClassId(11);
pub const REGEXP_CLASS: ClassId = ClassId(12);
pub const MATCH_DATA_CLASS: ClassId = ClassId(13);
pub const FIBER_CLASS: ClassId = ClassId(14);
pub const THREAD_CLASS: ClassId = ClassId(15);
pub const MUTEX_CLASS: ClassId = ClassId(16);
pub const QUEUE_CLASS: ClassId = ClassId(17);
pub const RACTOR_CLASS: ClassId = ClassId(18);
/// The builtin `Enumerable` MODULE -- implemented in Rust in
/// `zeo_rt::builtins::enumerable` (the enum.c architecture); `include
/// Enumerable` linearizes this id into a class's `ancestors` exactly like
/// a user module.
pub const ENUMERABLE_CLASS: ClassId = ClassId(19);
/// `Class` and `Module` -- the classes a first-class
/// class/module VALUE (`RubyValue::Class`) answers `.class` with:
/// `Widget.class == Class`, `Enumerable.class == Module`. Both are
/// themselves CLASSES (`Class.class == Class` in real Ruby); `Class`'s
/// superclass is `Module` (`Widget.is_a?(Module)` is true).
pub const CLASS_CLASS: ClassId = ClassId(20);
pub const MODULE_CLASS: ClassId = ClassId(21);
/// The builtin `Comparable` MODULE -- every method drives the
/// includer's own `<=>` (the compar.c architecture).
pub const COMPARABLE_CLASS: ClassId = ClassId(22);
/// Reserved for the fiber-backed Enumerator; registered with the
/// CRuby-correct ancestry now so ids stay append-only.
pub const ENUMERATOR_CLASS: ClassId = ClassId(23);
/// The true root: `BasicObject.superclass` is nil in Ruby;
/// every chain ends `..., Object, Kernel, BasicObject`.
pub const BASIC_OBJECT_CLASS: ClassId = ClassId(24);
/// The `Kernel` MODULE -- Object owns ZERO instance methods in CRuby;
/// every "universal" method (`class`, `dup`, `inspect`, `is_a?`, ...) is
/// Kernel's, mixed into Object.
pub const KERNEL_CLASS: ClassId = ClassId(25);
/// The numeric tower root: `Integer`/`Float`/`Rational`/`Complex` <
/// `Numeric`, which includes `Comparable`.
pub const NUMERIC_CLASS: ClassId = ClassId(26);
pub const RATIONAL_CLASS: ClassId = ClassId(27);
pub const COMPLEX_CLASS: ClassId = ClassId(28);
/// The `Math` MODULE (module functions `Math.sqrt` etc. + `PI`/`E`).
pub const MATH_CLASS: ClassId = ClassId(29);
/// `Struct` -- root of every runtime-minted `Struct.new(...)` class
/// (`zeo-rt`'s `rstruct`); includes `Enumerable` (CRuby).
pub const STRUCT_CLASS: ClassId = ClassId(30);
/// `Enumerator::Yielder` -- the `y` in
/// `Enumerator.new { |y| y << 1 }`. Registered under its FLAT
/// fully-qualified name (this table has no nesting edges); user code
/// resolving the `Enumerator::Yielder` path lexically gets a loud
/// NameError -- documented, since yielders are only ever OBTAINED, never
/// named.
pub const YIELDER_CLASS: ClassId = ClassId(31);
/// The `GC` MODULE -- zeo uses `Arc` refcounting, so
/// `GC.start`/`stat`/`enable`/`disable`/`compact` are honest no-ops (see
/// `zeo_rt::dispatch`'s GC probe); the id exists so `GC` resolves as a
/// constant and `GC.start` dispatches cleanly instead of NameError-ing.
pub const GC_CLASS: ClassId = ClassId(32);
/// `IO` (minimal) -- backs the `STDOUT`/`STDERR` singletons and the
/// `$stdout`/`$stderr` globals; the print family routes through whichever
/// value those globals hold.
pub const IO_CLASS: ClassId = ClassId(33);
/// `Method` -- the object `Kernel#method(:name)` answers; wraps a
/// bound receiver + method name and dispatches `#call` through `send`.
pub const METHOD_CLASS: ClassId = ClassId(34);

/// Core classes. All are `RubyValue::Object(RObj)` over a
/// zeo-rt-resident struct -- `RubyValue` stays frozen at its 23 variants
/// (a new variant only pays for itself for structural Hash-key equality, a
/// codegen fast path, or an immediate; none of these qualify).
///
/// `File < IO` is CRuby's real edge, so a `File` instance answers every IO
/// instance method through the ordinary MRO walk with no duplication.
pub const FILE_CLASS: ClassId = ClassId(35);
pub const DIR_CLASS: ClassId = ClassId(36);
/// `Time` includes `Comparable` (CRuby), so `t1 < t2`/`between?`/`clamp`
/// all fall out of the existing `comparable_send` driver once `Time#<=>`
/// exists.
pub const TIME_CLASS: ClassId = ClassId(37);
/// `Process` is a MODULE (`Process.pid`, `Process::CLOCK_MONOTONIC`).
pub const PROCESS_CLASS: ClassId = ClassId(38);
/// `File::Stat` -- what `File.stat`/`File#stat` answer; the predicates
/// (`File.file?`, `.directory?`, `.size`) read through it.
pub const FILE_STAT_CLASS: ClassId = ClassId(39);
/// `Encoding` -- what `String#encoding` answers and `Encoding::UTF_8` names;
/// wraps an `encoding::EncodingId` in the runtime.
pub const ENCODING_CLASS: ClassId = ClassId(40);
/// `Data` -- root of every runtime-minted `Data.define(...)` class
/// (`zeo-rt`'s `rstruct`). Unlike `Struct`, `Data` is immutable and NOT
/// `Enumerable` (no `each`). The one other subclassable builtin besides
/// `Struct`.
pub const DATA_CLASS: ClassId = ClassId(41);
pub const SET_CLASS: ClassId = ClassId(42);
pub const LAZY_CLASS: ClassId = ClassId(43);
/// Ruby's `Thread::ConditionVariable` (exposed top-level as `ConditionVariable`,
/// matching how `Mutex`/`Queue` are already simplified from `Thread::*`).
pub const CONDITION_VARIABLE_CLASS: ClassId = ClassId(44);
/// `Module#instance_method`'s result -- a `Method` not yet bound to a receiver.
pub const UNBOUND_METHOD_CLASS: ClassId = ClassId(45);
/// The `base64` extension's `Base64` module (require-gated, in-tree `ext/`).
pub const BASE64_MODULE: ClassId = ClassId(46);
/// `stringio`: an in-memory `IO`-like bytes buffer.
pub const STRINGIO_CLASS: ClassId = ClassId(47);
/// `strscan`: `StringScanner`, a position-tracking lexer over a String.
pub const STRING_SCANNER_CLASS: ClassId = ClassId(48);
/// `cgi/escape`: the `CGI` module's URL/HTML escape helpers.
pub const CGI_MODULE: ClassId = ClassId(49);
/// `digest`: the `Digest` framework module and its algorithm classes below.
pub const DIGEST_MODULE: ClassId = ClassId(50);
pub const DIGEST_MD5_CLASS: ClassId = ClassId(51);
pub const DIGEST_SHA1_CLASS: ClassId = ClassId(52);
pub const DIGEST_SHA256_CLASS: ClassId = ClassId(53);
pub const DIGEST_SHA512_CLASS: ClassId = ClassId(54);
/// `json`: the `JSON` module (parser/generator). Scaffolded (see docs/EXTENSIONS.md).
pub const JSON_MODULE: ClassId = ClassId(55);
/// `date`: `Date`/`DateTime`. Scaffolded.
pub const DATE_CLASS: ClassId = ClassId(56);
pub const DATETIME_CLASS: ClassId = ClassId(57);
/// `zlib`: the `Zlib` compression module. Scaffolded.
pub const ZLIB_MODULE: ClassId = ClassId(58);
/// `psych`: the `Psych` YAML module. Scaffolded.
pub const PSYCH_MODULE: ClassId = ClassId(59);
/// `socket`: the `Socket` class. Scaffolded.
pub const SOCKET_CLASS: ClassId = ClassId(60);
/// `openssl`: the `OpenSSL` module. Scaffolded.
pub const OPENSSL_MODULE: ClassId = ClassId(61);
/// `yaml`: the `YAML` module -- an alias for `Psych` (Ruby's `yaml.rb` does
/// `YAML = Psych`), so it shares the `psych` feature and dispatch.
pub const YAML_MODULE: ClassId = ClassId(62);
/// `Random` -- a seedable PRNG. An ordinary always-on core class (not an
/// ext), appended at the end of the builtin id block.
pub const RANDOM_CLASS: ClassId = ClassId(63);

/// `SizedQueue < Queue` -- a bounded blocking queue. Shares the whole `Queue`
/// method table via the ancestor chain, adding only `max`/`max=`; its
/// instances are `RubyValue::Queue` values whose runtime payload carries the
/// bound (see `zeo_rt::queue_is_sized`).
pub const SIZED_QUEUE_CLASS: ClassId = ClassId(64);

/// `Marshal` -- the object-serialization module (`Marshal.dump`/`.load`).
/// Always-on; accessed only through its class methods.
pub const MARSHAL_MODULE: ClassId = ClassId(65);

/// `Monitor` -- the reentrant lock from `require "monitor"`. Unlike `Mutex`,
/// the owning execution may re-enter it; see `ext::monitor`.
pub const MONITOR_CLASS: ClassId = ClassId(66);

/// `Process::Status` -- the wait-status object a `system`/backtick leaves in
/// `$?`. A core class (always present, no feature gate); see
/// `builtins::process`.
pub const PROCESS_STATUS_CLASS: ClassId = ClassId(67);

/// The `FFI` module (`require "ffi"`, the real `ffi` gem). Require-gated
/// like the `ext/` classes but recognized as a first-class runtime namespace so
/// its `Pointer`/`MemoryPointer`/`Struct` constants resolve. `FFI::Library` is
/// NOT a row -- it's recognized syntactically (`extend FFI::Library`), never a
/// runtime constant.
pub const FFI_MODULE: ClassId = ClassId(68);
/// `FFI::Pointer` -- a wrapped C address with typed read/write accessors. See
/// `ext::ffi`.
pub const FFI_POINTER_CLASS: ClassId = ClassId(69);
/// `FFI::MemoryPointer < FFI::Pointer` -- a pointer that OWNS a heap buffer it
/// allocated; inherits every read/write accessor from `Pointer` via the
/// ancestor chain.
pub const FFI_MEMORY_POINTER_CLASS: ClassId = ClassId(70);
/// `FFI::Struct` -- root of every `class T < FFI::Struct; layout ...; end`. A
/// native-backed subclassable builtin (like `Struct`); its `layout` is
/// recognized at compile time.
pub const FFI_STRUCT_CLASS: ClassId = ClassId(71);

/// The `Signal` MODULE (`Signal.list`/`signame`/`trap`). Core and always-on
/// (CRuby exposes it unconditionally), like `Process`/`Math`.
pub const SIGNAL_MODULE: ClassId = ClassId(72);

/// The `FileTest` MODULE -- CRuby's mixin of the pure `File` predicates
/// (`exist?`/`file?`/`directory?`/...). Its module functions ARE `File`'s
/// class methods (CRuby shares one C implementation), so the runtime reuses
/// `file::lookup_class` for it rather than a parallel table.
pub const FILE_TEST_MODULE: ClassId = ClassId(73);

/// The class of the singleton `ARGF` object. CRuby names it literally
/// `"ARGF.class"` (so `ARGF.class.to_s == "ARGF.class"`); it includes
/// `Enumerable` and is never user-instantiated.
pub const ARGF_CLASS: ClassId = ClassId(74);

/// `Enumerator::Chain` -- what `Enumerator#+` and `Enumerable#chain` answer.
/// A chain IS an Enumerator (CRuby: `Enumerator::Chain.superclass ==
/// Enumerator`), so the runtime carries it as a `RubyValue::Enumerator` over
/// an `EnumSource::Chain` and reports this id from `class_of` instead of
/// giving it a separate payload type.
pub const ENUMERATOR_CHAIN_CLASS: ClassId = ClassId(75);

/// `Enumerator::Product` -- what `Enumerator.product` answers. Carried the
/// same way as [`ENUMERATOR_CHAIN_CLASS`].
pub const ENUMERATOR_PRODUCT_CLASS: ClassId = ClassId(76);

/// `Process::Tms` -- the CPU-times struct `Process.times` answers, with
/// Float members `utime`/`stime`/`cutime`/`cstime`. Carried as a plain
/// `RubyValue::Object` over `builtins::process::RTms`.
pub const PROCESS_TMS_CLASS: ClassId = ClassId(77);

/// `TCPSocket` (`require "socket"`) -- a connected TCP stream. Backed by an
/// `RIo` over the socket fd, so it inherits IO's read/write/gets surface
/// (`TCPSocket < IO`). Feature-gated on `socket`.
pub const TCPSOCKET_CLASS: ClassId = ClassId(78);

/// `TCPServer` (`require "socket"`) -- a listening TCP socket. `TCPServer <
/// TCPSocket` (CRuby's hierarchy); adds `accept`/`addr`. Any new [`BUILTINS`]
/// row must be APPENDED after this so ids stay contiguous and the exception
/// block (`FIRST_EXCEPTION_ID`) follows the last builtin.
pub const TCPSERVER_CLASS: ClassId = ClassId(79);

/// `ThreadGroup` -- always-on core (no require), like `Thread` itself. The
/// runtime models the DEFAULT group only (every thread belongs to
/// `ThreadGroup::Default`; `enclose`/re-grouping are not implemented) --
/// enough for the stdlib idiom of checking `thread.group.enclosed?` and
/// re-adding to `ThreadGroup::Default` (timeout does both).
pub const THREAD_GROUP_CLASS: ClassId = ClassId(80);

/// The `Warning` module -- category flags (`Warning[:deprecated]` /
/// `[]=`) and the `Warning.warn` sink. Always-on core: stdlib probes it
/// at load time (ostruct's `HAS_PERFORMANCE_WARNINGS`).
pub const WARNING_MODULE: ClassId = ClassId(81);

/// The `ObjectSpace` module -- `define_finalizer`/`garbage_collect`/
/// `count_objects`, and the honest-`NotImplementedError` `each_object`/
/// `_id2ref` (zeo's `Arc` model can't enumerate the live heap or resolve an
/// id back to an object). Namespaces `ObjectSpace::WeakMap`.
pub const OBJECTSPACE_MODULE: ClassId = ClassId(82);

/// `ObjectSpace::WeakMap` -- an identity-keyed map holding weak references to
/// its keys and values (`Arc::downgrade`); dead entries are pruned on access
/// and at `GC.start`. The primitive `WeakRef` builds on.
pub const WEAKMAP_CLASS: ClassId = ClassId(83);

/// `WeakRef` -- a weak-reference delegator: forwards methods to its referent
/// while it lives, raising `WeakRef::RefError` once it's been collected.
/// CRuby roots it at `Delegator < BasicObject`, which zeo doesn't model;
/// `Object` is the pragmatic parent.
pub const WEAKREF_CLASS: ClassId = ClassId(84);

/// `Random::Formatter` -- the mixin (CRuby's `random/formatter.rb`) that turns
/// a source of random bytes into `hex`/`base64`/`urlsafe_base64`/`uuid`/
/// `random_number`/`alphanumeric`. `require`-gated on `"random/formatter"`; the
/// same module both `SecureRandom` and rubygems' vendored `Gem::SecureRandom`
/// `extend`. Its methods draw bytes by re-dispatching `gen_random` to the
/// receiver (like `Comparable` drives the receiver's `<=>`), so one native
/// impl serves every host module.
pub const RANDOM_FORMATTER_MODULE: ClassId = ClassId(85);

/// The `Etc` module (CRuby's `ext/etc`) -- access to the system user/group
/// databases, `sysconf`/`confstr`, `uname`, `nprocessors`, and the install-path
/// constants. `require`-gated on `"etc"`. Namespaces `Etc::Passwd`/`Etc::Group`.
pub const ETC_MODULE: ClassId = ClassId(86);

/// `Etc::Passwd` -- one row of the system password database (name/uid/gid/dir/
/// shell/...), what `Etc.getpwnam`/`getpwuid`/`getpwent` answer.
pub const ETC_PASSWD_CLASS: ClassId = ClassId(87);

/// `Etc::Group` -- one row of the system group database (name/gid/members),
/// what `Etc.getgrnam`/`getgrgid`/`getgrent` answer.
pub const ETC_GROUP_CLASS: ClassId = ClassId(88);

/// `Pathname` -- the `pathname` stdlib class (a value wrapping a path String).
/// `require`-gated on `"pathname"`; a focused native implementation over
/// `File`/`Dir`/`std::path`.
pub const PATHNAME_CLASS: ClassId = ClassId(89);

/// The `socket` gem's socket hierarchy, mirroring CRuby's:
/// `BasicSocket < IO`, `IPSocket < BasicSocket`, `TCPSocket < IPSocket`,
/// `TCPServer < TCPSocket`, `UDPSocket < IPSocket`, `Socket < BasicSocket`,
/// `UNIXSocket < BasicSocket`, `UNIXServer < UNIXSocket`. All the fd-backed
/// classes descend from `BasicSocket`, which owns the raw-fd operations
/// (`getsockname`/`setsockopt`/`send`/`recv`/...) they all share.
pub const BASIC_SOCKET_CLASS: ClassId = ClassId(90);
/// `IPSocket < BasicSocket` -- the shared `addr`/`peeraddr`/`recvfrom` of the
/// IP-family sockets (`TCPSocket`, `UDPSocket`).
pub const IP_SOCKET_CLASS: ClassId = ClassId(91);
/// `UDPSocket < IPSocket` -- a connectionless datagram socket.
pub const UDP_SOCKET_CLASS: ClassId = ClassId(92);
/// `UNIXSocket < BasicSocket` -- a local (AF_UNIX) stream socket.
pub const UNIX_SOCKET_CLASS: ClassId = ClassId(93);
/// `UNIXServer < UNIXSocket` -- a listening local socket.
pub const UNIX_SERVER_CLASS: ClassId = ClassId(94);
/// `Addrinfo` -- a resolved socket address (family/type/protocol + endpoint),
/// what `getaddrinfo`/`#local_address`/`#remote_address` answer.
pub const ADDRINFO_CLASS: ClassId = ClassId(95);
/// `Thread::Backtrace::Location` -- one entry of a backtrace as an OBJECT
/// (`#path`/`#lineno`/`#label`), what `Kernel#caller_locations` answers and
/// `Exception#backtrace_locations` would.
pub const BACKTRACE_LOCATION_CLASS: ClassId = ClassId(96);
/// `fcntl`: the `Fcntl` module's `fcntl(2)`/`open(2)` flag constants. No
/// methods -- CRuby's extension is a constant table, and `IO#fcntl` is IO's.
pub const FCNTL_MODULE: ClassId = ClassId(97);
/// `IO::ConsoleMode` -- a saved terminal mode, what `IO#console_mode` hands
/// back and `IO#console_mode=` restores. Not constructible from Ruby (CRuby's
/// has no `initialize` either); its `raw`/`raw!`/`echo=` rows edit the saved
/// mode before it is put back.
pub const CONSOLE_MODE_CLASS: ClassId = ClassId(98);

/// `zlib`'s stream classes, mirroring CRuby's:
///
/// ```text
/// Zlib::ZStream          the shared counters/lifecycle (never constructed)
///  ├ Zlib::Deflate       a compressor
///  └ Zlib::Inflate       a decompressor
/// Zlib::GzipFile         the shared gzip header/footer accessors
///  ├ Zlib::GzipWriter    writes a gzip member to an IO
///  └ Zlib::GzipReader    reads a gzip member from an IO (includes Enumerable)
/// ```
///
/// `Zlib`'s thirteen exception classes are NOT here -- a gated row registers
/// no constructor, so they live in `gems/zlib/lib/zlib.rb` (see `ext/mod.rs`).
pub const ZLIB_ZSTREAM_CLASS: ClassId = ClassId(99);
/// `Zlib::Deflate < Zlib::ZStream` -- a compressor.
pub const ZLIB_DEFLATE_CLASS: ClassId = ClassId(100);
/// `Zlib::Inflate < Zlib::ZStream` -- a decompressor.
pub const ZLIB_INFLATE_CLASS: ClassId = ClassId(101);
/// `Zlib::GzipFile` -- the gzip header/footer surface both directions share.
pub const ZLIB_GZIP_FILE_CLASS: ClassId = ClassId(102);
/// `Zlib::GzipWriter < Zlib::GzipFile` -- an IO-shaped gzip compressor.
pub const ZLIB_GZIP_WRITER_CLASS: ClassId = ClassId(103);
/// `Zlib::GzipReader < Zlib::GzipFile` -- an IO-shaped gzip decompressor.
pub const ZLIB_GZIP_READER_CLASS: ClassId = ClassId(104);
/// `pty`: the `PTY` module -- pseudo-terminal allocation (`.open`) and
/// child processes run under one (`.spawn`/`.getpty`, `.check`). Its
/// `ChildExited` exception lives in the gem's Ruby half (`gems/pty`), the
/// same split as `Zlib`'s errors.
pub const PTY_MODULE: ClassId = ClassId(105);
/// `syslog`: the `Syslog` module over the system `syslog(3)` facility --
/// `open`/`log`/`mask` plus the priority/facility/option constant set. Its
/// `Constants`/`Level`/`Option`/`Facility`/`Macros` submodules live in the
/// gem's Ruby half (`gems/syslog`).
pub const SYSLOG_MODULE: ClassId = ClassId(106);
/// `readline`: the `Readline` module -- `readline` line input (rustyline on a
/// terminal, a plain read everywhere else) plus the completion/word-break
/// attribute surface.
pub const READLINE_MODULE: ClassId = ClassId(107);
/// The class of the `Readline::HISTORY` singleton -- the one history list as
/// an Enumerable object (`push`/`<<`/`[]`/`delete_at`/`each`/...). Not
/// constructible from Ruby. Deliberately NOT named `Readline::HISTORY`: the
/// compiler resolves a builtin's name as a CONSTANT PATH, and that spelling
/// must resolve to the runtime-seeded singleton OBJECT, not to its class.
pub const READLINE_HISTORY_CLASS: ClassId = ClassId(108);
/// `nkf`: the `NKF` module -- Network Kanji Filter, Japanese text encoding
/// conversion (`.nkf` over an option string, `.guess`) rebuilt over the
/// runtime's own encoding engine. The `Kconv` wrapper is the gem's Ruby
/// half (`gems/nkf`).
pub const NKF_MODULE: ClassId = ClassId(109);
/// `bigdecimal`: the `BigDecimal` class -- arbitrary-precision decimal
/// arithmetic. The native half is bigdecimal 4.x's C slice (exact
/// arithmetic, division precision, rounding, mode state); `power`/`sqrt`/
/// `BigMath` and the `to_d` family are the gem's own Ruby, vendored in
/// `gems/bigdecimal`. Not constructible via `new` (CRuby removed it); the
/// `Kernel#BigDecimal` function is the one constructor.
pub const BIGDECIMAL_CLASS: ClassId = ClassId(110);

/// `FFI::Type` -- the ffi gem's type objects (`FFI::Type::INT32.size`).
/// The canonical instances live on this class and `Builtin` as constants;
/// `fiddle`'s FFI backend keys its whole type table off them.
pub const FFI_TYPE_CLASS: ClassId = ClassId(111);
/// `FFI::Type::Builtin < FFI::Type` -- the class of the canonical scalar
/// type instances (`FFI::Type::Builtin::VOID`, `::POINTER`, ...).
pub const FFI_TYPE_BUILTIN_CLASS: ClassId = ClassId(112);
/// `FFI::DynamicLibrary` -- `dlopen(3)` handles: `.open(name, flags)` and
/// `#find_function` over `dlsym`, with the `RTLD_*` constants.
pub const FFI_DYNAMIC_LIBRARY_CLASS: ClassId = ClassId(113);
/// `FFI::Function < FFI::Pointer` -- a callable C function pointer built at
/// RUNTIME (libffi): from a code address, or from a Ruby `Proc` (a closure
/// trampoline). The compile-time `attach_function` path never constructs one;
/// `fiddle` is the consumer.
pub const FFI_FUNCTION_CLASS: ClassId = ClassId(114);
/// `FFI::VariadicInvoker` -- the runtime call builder for a variadic C
/// function; each `#call` marshals trailing `(type, value)` pairs.
pub const FFI_VARIADIC_INVOKER_CLASS: ClassId = ClassId(115);
/// `FFI::AbstractMemory` -- the gem's abstract base of `Pointer`/`Buffer`.
/// Constant-only here (never instantiated): it exists so `is_a?` checks in
/// the gem's own Ruby (fiddle's FFI backend) answer correctly.
pub const FFI_ABSTRACT_MEMORY_CLASS: ClassId = ClassId(116);
/// `FFI::AutoPointer < FFI::Pointer` -- constant-only, for `is_a?` checks.
pub const FFI_AUTO_POINTER_CLASS: ClassId = ClassId(117);

/// `coverage`: the `Coverage` MODULE -- line-coverage measurement over the
/// AOT line instrumentation (the compiler emits per-statement hit counters
/// and a coverable-line table when a program requires it). Lines only;
/// `supported?(:branches)`/`(:methods)` answer false.
pub const COVERAGE_MODULE: ClassId = ClassId(118);

/// `TracePoint` -- execution tracing over the runtime's frame/line
/// instrumentation (`set_line`, `FrameGuard`). Core (require-less), so
/// ungated; the runtime half lives behind the `ext-tracepoint` cargo
/// feature. `:line`/`:call`/`:return`/`:class`/`:end`/`:raise` only.
pub const TRACEPOINT_CLASS: ClassId = ClassId(119);

/// The `openssl` class surface (the `OpenSSL` module itself is
/// [`OPENSSL_MODULE`], an early row). Backed by the vendored OpenSSL 3.x the
/// `openssl` crate builds, so the digest/cipher/BN/TLS behavior is CRuby's
/// own EVP implementations. CRuby parents `OpenSSL::Digest` under the
/// `digest` framework's `Digest::Class`; zeo's digest classes are native
/// tables with no shared Ruby superclass, so it sits under `Object` (a
/// documented divergence). The exception hierarchy lives in
/// `gems/openssl/lib` -- see `ext/mod.rs` on why a gated native class
/// cannot register a constructible exception.
pub const OPENSSL_DIGEST_CLASS: ClassId = ClassId(120);
/// `OpenSSL::Digest`'s fixed algorithm subclasses, one id each, all served
/// by the shared digest table (the `Digest::MD5`-style aliasing).
pub const OPENSSL_DIGEST_MD4_CLASS: ClassId = ClassId(121);
pub const OPENSSL_DIGEST_MD5_CLASS: ClassId = ClassId(122);
pub const OPENSSL_DIGEST_RIPEMD160_CLASS: ClassId = ClassId(123);
pub const OPENSSL_DIGEST_SHA1_CLASS: ClassId = ClassId(124);
pub const OPENSSL_DIGEST_SHA224_CLASS: ClassId = ClassId(125);
pub const OPENSSL_DIGEST_SHA256_CLASS: ClassId = ClassId(126);
pub const OPENSSL_DIGEST_SHA384_CLASS: ClassId = ClassId(127);
pub const OPENSSL_DIGEST_SHA512_CLASS: ClassId = ClassId(128);
/// `OpenSSL::HMAC` -- streaming keyed MAC.
pub const OPENSSL_HMAC_CLASS: ClassId = ClassId(129);
/// `OpenSSL::KDF` -- `pbkdf2_hmac`/`hkdf`/`scrypt` module functions.
pub const OPENSSL_KDF_MODULE: ClassId = ClassId(130);
/// `OpenSSL::BN` -- OpenSSL's BIGNUM.
pub const OPENSSL_BN_CLASS: ClassId = ClassId(131);
/// `OpenSSL::Cipher` -- the EVP symmetric-cipher surface.
pub const OPENSSL_CIPHER_CLASS: ClassId = ClassId(132);
/// `OpenSSL::SSL` -- the TLS module (constants + the two classes below).
pub const OPENSSL_SSL_MODULE: ClassId = ClassId(133);
/// `OpenSSL::SSL::SSLContext` -- client-side TLS configuration.
pub const OPENSSL_SSL_CONTEXT_CLASS: ClassId = ClassId(134);
/// `OpenSSL::SSL::SSLSocket` -- a client TLS session over an IO.
pub const OPENSSL_SSL_SOCKET_CLASS: ClassId = ClassId(135);
/// `OpenSSL::X509` -- the certificate module (verify-side only: zeo ships
/// no issuance).
pub const OPENSSL_X509_MODULE: ClassId = ClassId(136);
/// `OpenSSL::X509::Store` -- the CA trust store a context verifies against.
pub const OPENSSL_X509_STORE_CLASS: ClassId = ClassId(137);
/// `OpenSSL::X509::Certificate` -- a parsed certificate (peer certs).
pub const OPENSSL_X509_CERT_CLASS: ClassId = ClassId(138);
/// `OpenSSL::X509::Name` -- a certificate subject/issuer DN.
pub const OPENSSL_X509_NAME_CLASS: ClassId = ClassId(139);
/// `OpenSSL::Random` -- CSPRNG bytes (`random_bytes`).
pub const OPENSSL_RANDOM_MODULE: ClassId = ClassId(140);
/// `Binding` -- a captured scope: its `self`, its locals (shared cells the
/// compiled frame keeps writing to), and the lexical context a `def` or a
/// constant inside `Binding#eval` resolves against.
pub const BINDING_CLASS: ClassId = ClassId(141);

/// `OpenSSL::Buffering` -- the buffered IO surface CRuby mixes into
/// `SSLSocket` over its `sysread`/`syswrite`/`sysclose`.
pub const OPENSSL_BUFFERING_MODULE: ClassId = ClassId(143);
/// `OpenSSL::SSL::SocketForwarder` -- the descriptor-level questions
/// `SSLSocket` passes down to the socket underneath.
pub const OPENSSL_SOCKET_FORWARDER_MODULE: ClassId = ClassId(144);

/// `Prism` -- the vendored gem's namespace. A builtin so its NATIVE half
/// below can nest inside it; the gem's Ruby then reopens it, the same split
/// `Zlib` makes.
pub const PRISM_MODULE: ClassId = ClassId(146);

/// `File::Constants` -- the open/lock/fnmatch flags. A MODULE rather than a
/// bag of constants on `File`, because CRuby includes it into `IO` as well:
/// that is what makes `IO::APPEND` and `File::APPEND` the same constant, and
/// what puts `File::Constants` in `IO.ancestors`.
pub const FILE_CONSTANTS_MODULE: ClassId = ClassId(147);

/// `Enumerator::ArithmeticSequence` -- what a BLOCKLESS `Range#step`,
/// `Range#%` or `Numeric#step` over a numeric receiver answers. An
/// `Enumerator` that also carries its `(begin, end, step, exclude_end)`
/// quadruple, so `#size`/`#last` compute rather than iterate and `Array#[]`
/// can slice with a stride.
pub const ENUMERATOR_ARITHMETIC_SEQUENCE_CLASS: ClassId = ClassId(148);

/// `Process::Sys` -- the raw `set*id` syscalls, one module function apiece,
/// each failing with the matching `Errno::*`.
pub const PROCESS_SYS_MODULE: ClassId = ClassId(149);

/// `Process::UID` and `Process::GID` -- the privilege API CRuby layers over
/// `Process::Sys`, where the same ten names read and switch the user's and
/// the group's ids.
pub const PROCESS_UID_MODULE: ClassId = ClassId(150);
pub const PROCESS_GID_MODULE: ClassId = ClassId(151);

/// `Refinement` -- what `M.refinements` holds and what a refined method's
/// `Method#owner` reports. A `Module` subclass with no instances of its
/// own here: the compiler mints one hidden module per `refine` block and
/// MARKS it, which is what makes `owner.class` answer `Refinement` while
/// `Module` still answers for every ordinary module.
pub const REFINEMENT_CLASS: ClassId = ClassId(145);

/// `Socket::Option` -- one socket option's `(family, level, optname, data)`,
/// which `BasicSocket#getsockopt` answers.
pub const SOCKET_OPTION_CLASS: ClassId = ClassId(142);

/// Every reserved built-in class/module except `Object` (see
/// [`OBJECT_CLASS`]), in id order -- ids are contiguous from 1 by
/// construction (asserted by the unit test below), which is what lets the
/// compiler seed its class arena by pushing these in order. Superclass
/// edges may point FORWARD in the table (`Integer(1)` -> `Numeric(26)`);
/// consumers store the edge and linearize later.
pub const BUILTINS: &[BuiltinClass] = &[
    BuiltinClass {
        id: INTEGER_CLASS,
        name: "Integer",
        is_module: false,
        superclass: Some(NUMERIC_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: FLOAT_CLASS,
        name: "Float",
        is_module: false,
        superclass: Some(NUMERIC_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: STRING_CLASS,
        name: "String",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[COMPARABLE_CLASS],
        feature: None,
    },
    BuiltinClass {
        id: SYMBOL_CLASS,
        name: "Symbol",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[COMPARABLE_CLASS],
        feature: None,
    },
    BuiltinClass {
        id: ARRAY_CLASS,
        name: "Array",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[ENUMERABLE_CLASS],
        feature: None,
    },
    BuiltinClass {
        id: HASH_CLASS,
        name: "Hash",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[ENUMERABLE_CLASS],
        feature: None,
    },
    BuiltinClass {
        id: RANGE_CLASS,
        name: "Range",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[ENUMERABLE_CLASS],
        feature: None,
    },
    BuiltinClass {
        id: NIL_CLASS,
        name: "NilClass",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: TRUE_CLASS,
        name: "TrueClass",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: FALSE_CLASS,
        name: "FalseClass",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: PROC_CLASS,
        name: "Proc",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: REGEXP_CLASS,
        name: "Regexp",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: MATCH_DATA_CLASS,
        name: "MatchData",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: FIBER_CLASS,
        name: "Fiber",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: THREAD_CLASS,
        name: "Thread",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: MUTEX_CLASS,
        name: "Thread::Mutex",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: QUEUE_CLASS,
        name: "Thread::Queue",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: RACTOR_CLASS,
        name: "Ractor",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: ENUMERABLE_CLASS,
        name: "Enumerable",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: CLASS_CLASS,
        name: "Class",
        is_module: false,
        superclass: Some(MODULE_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: MODULE_CLASS,
        name: "Module",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: COMPARABLE_CLASS,
        name: "Comparable",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: ENUMERATOR_CLASS,
        name: "Enumerator",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[ENUMERABLE_CLASS],
        feature: None,
    },
    BuiltinClass {
        id: BASIC_OBJECT_CLASS,
        name: "BasicObject",
        is_module: false,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: KERNEL_CLASS,
        name: "Kernel",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: NUMERIC_CLASS,
        name: "Numeric",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[COMPARABLE_CLASS],
        feature: None,
    },
    BuiltinClass {
        id: RATIONAL_CLASS,
        name: "Rational",
        is_module: false,
        superclass: Some(NUMERIC_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: COMPLEX_CLASS,
        name: "Complex",
        is_module: false,
        superclass: Some(NUMERIC_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: MATH_CLASS,
        name: "Math",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: STRUCT_CLASS,
        name: "Struct",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[ENUMERABLE_CLASS],
        feature: None,
    },
    BuiltinClass {
        id: YIELDER_CLASS,
        name: "Enumerator::Yielder",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: GC_CLASS,
        name: "GC",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: IO_CLASS,
        name: "IO",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        // Reverse of the resolution order, as `include A; include B` is:
        // CRuby's `IO.ancestors` is `[IO, File::Constants, Enumerable, ...]`.
        includes: &[ENUMERABLE_CLASS, FILE_CONSTANTS_MODULE],
        feature: None,
    },
    BuiltinClass {
        id: METHOD_CLASS,
        name: "Method",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: FILE_CLASS,
        name: "File",
        is_module: false,
        superclass: Some(IO_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: DIR_CLASS,
        name: "Dir",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[ENUMERABLE_CLASS],
        feature: None,
    },
    BuiltinClass {
        id: TIME_CLASS,
        name: "Time",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[COMPARABLE_CLASS],
        feature: None,
    },
    BuiltinClass {
        id: PROCESS_CLASS,
        name: "Process",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: FILE_STAT_CLASS,
        name: "File::Stat",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[COMPARABLE_CLASS],
        feature: None,
    },
    BuiltinClass {
        id: ENCODING_CLASS,
        name: "Encoding",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: DATA_CLASS,
        name: "Data",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: SET_CLASS,
        name: "Set",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[ENUMERABLE_CLASS],
        feature: None,
    },
    BuiltinClass {
        id: LAZY_CLASS,
        name: "Enumerator::Lazy",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[ENUMERABLE_CLASS],
        feature: None,
    },
    BuiltinClass {
        id: CONDITION_VARIABLE_CLASS,
        name: "Thread::ConditionVariable",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: UNBOUND_METHOD_CLASS,
        name: "UnboundMethod",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: BASE64_MODULE,
        name: "Base64",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("base64"),
    },
    // In-tree `ext/` extensions -- CRuby's ext/ model. Each is require-gated
    // (its constant is invisible until its `require` fires) AND compile-gated
    // by a per-extension cargo feature on `zeo-rt` (see that crate's
    // `[features]` and `ext/mod.rs`). Some carry real implementations, others
    // are scaffolded (a couple methods, the rest `todo!`) -- see
    // `docs/EXTENSIONS.md` for the per-extension status.
    BuiltinClass {
        id: STRINGIO_CLASS,
        name: "StringIO",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[ENUMERABLE_CLASS],
        feature: Some("stringio"),
    },
    BuiltinClass {
        id: STRING_SCANNER_CLASS,
        name: "StringScanner",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("strscan"),
    },
    BuiltinClass {
        id: CGI_MODULE,
        name: "CGI",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("cgi/escape"),
    },
    BuiltinClass {
        id: DIGEST_MODULE,
        name: "Digest",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("digest"),
    },
    BuiltinClass {
        id: DIGEST_MD5_CLASS,
        name: "Digest::MD5",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("digest"),
    },
    BuiltinClass {
        id: DIGEST_SHA1_CLASS,
        name: "Digest::SHA1",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("digest"),
    },
    BuiltinClass {
        id: DIGEST_SHA256_CLASS,
        name: "Digest::SHA256",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("digest"),
    },
    BuiltinClass {
        id: DIGEST_SHA512_CLASS,
        name: "Digest::SHA512",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("digest"),
    },
    BuiltinClass {
        id: JSON_MODULE,
        name: "JSON",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("json"),
    },
    BuiltinClass {
        id: DATE_CLASS,
        name: "Date",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[COMPARABLE_CLASS],
        feature: Some("date"),
    },
    BuiltinClass {
        id: DATETIME_CLASS,
        name: "DateTime",
        is_module: false,
        superclass: Some(DATE_CLASS),
        includes: &[],
        feature: Some("date"),
    },
    BuiltinClass {
        id: ZLIB_MODULE,
        name: "Zlib",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("zlib"),
    },
    BuiltinClass {
        id: PSYCH_MODULE,
        name: "Psych",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("psych"),
    },
    BuiltinClass {
        id: SOCKET_CLASS,
        name: "Socket",
        is_module: false,
        superclass: Some(BASIC_SOCKET_CLASS),
        includes: &[],
        feature: Some("socket"),
    },
    BuiltinClass {
        id: OPENSSL_MODULE,
        name: "OpenSSL",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("openssl"),
    },
    // `YAML` is `Psych` under Ruby's `yaml.rb` (`YAML = Psych`); it shares the
    // `psych` feature so `require "yaml"` (canonicalized to `psych` by the
    // loader) makes both constants resolve, and both dispatch to `ext::psych`.
    BuiltinClass {
        id: YAML_MODULE,
        name: "YAML",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("psych"),
    },
    BuiltinClass {
        id: RANDOM_CLASS,
        name: "Random",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: SIZED_QUEUE_CLASS,
        name: "Thread::SizedQueue",
        is_module: false,
        superclass: Some(QUEUE_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: MARSHAL_MODULE,
        name: "Marshal",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: MONITOR_CLASS,
        name: "Monitor",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("monitor"),
    },
    BuiltinClass {
        id: PROCESS_STATUS_CLASS,
        name: "Process::Status",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    // The real `ffi` gem, require-gated on "ffi". The `FFI` module row
    // MUST precede its nested classes (nested-constant resolution walks parent
    // first). `MemoryPointer < Pointer` so it inherits Pointer's accessors.
    BuiltinClass {
        id: FFI_MODULE,
        name: "FFI",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("ffi"),
    },
    BuiltinClass {
        id: FFI_POINTER_CLASS,
        name: "FFI::Pointer",
        is_module: false,
        // The gem's hierarchy: `Pointer < AbstractMemory` (a forward edge --
        // the `AbstractMemory` row lives with the other late FFI ids).
        superclass: Some(FFI_ABSTRACT_MEMORY_CLASS),
        includes: &[],
        feature: Some("ffi"),
    },
    BuiltinClass {
        id: FFI_MEMORY_POINTER_CLASS,
        name: "FFI::MemoryPointer",
        is_module: false,
        superclass: Some(FFI_POINTER_CLASS),
        includes: &[],
        feature: Some("ffi"),
    },
    BuiltinClass {
        id: FFI_STRUCT_CLASS,
        name: "FFI::Struct",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("ffi"),
    },
    BuiltinClass {
        id: SIGNAL_MODULE,
        name: "Signal",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: FILE_TEST_MODULE,
        name: "FileTest",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: ARGF_CLASS,
        name: "ARGF.class",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[ENUMERABLE_CLASS],
        feature: None,
    },
    BuiltinClass {
        id: ENUMERATOR_CHAIN_CLASS,
        name: "Enumerator::Chain",
        is_module: false,
        superclass: Some(ENUMERATOR_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: ENUMERATOR_PRODUCT_CLASS,
        name: "Enumerator::Product",
        is_module: false,
        superclass: Some(ENUMERATOR_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: PROCESS_TMS_CLASS,
        name: "Process::Tms",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[COMPARABLE_CLASS],
        feature: None,
    },
    // `require "socket"`. TCPSocket < IPSocket (inherits BasicSocket's raw-fd
    // ops + IO's read/write/gets); TCPServer < TCPSocket (adds accept/addr).
    // The rest of the socket hierarchy (BasicSocket, IPSocket, ...) is appended
    // at the very end of the table (ids 90+).
    BuiltinClass {
        id: TCPSOCKET_CLASS,
        name: "TCPSocket",
        is_module: false,
        superclass: Some(IP_SOCKET_CLASS),
        includes: &[],
        feature: Some("socket"),
    },
    BuiltinClass {
        id: TCPSERVER_CLASS,
        name: "TCPServer",
        is_module: false,
        superclass: Some(TCPSOCKET_CLASS),
        includes: &[],
        feature: Some("socket"),
    },
    BuiltinClass {
        id: THREAD_GROUP_CLASS,
        name: "ThreadGroup",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: WARNING_MODULE,
        name: "Warning",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: OBJECTSPACE_MODULE,
        name: "ObjectSpace",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: WEAKMAP_CLASS,
        name: "ObjectSpace::WeakMap",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: WEAKREF_CLASS,
        name: "WeakRef",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: RANDOM_FORMATTER_MODULE,
        name: "Random::Formatter",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("random/formatter"),
    },
    BuiltinClass {
        id: ETC_MODULE,
        name: "Etc",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("etc"),
    },
    BuiltinClass {
        id: ETC_PASSWD_CLASS,
        name: "Etc::Passwd",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("etc"),
    },
    BuiltinClass {
        id: ETC_GROUP_CLASS,
        name: "Etc::Group",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("etc"),
    },
    BuiltinClass {
        id: PATHNAME_CLASS,
        name: "Pathname",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[COMPARABLE_CLASS],
        feature: Some("pathname"),
    },
    // The `socket` gem's hierarchy (all `require "socket"`-gated). Appended
    // after the last non-socket id so the table stays contiguous; the
    // forward edges from the earlier `Socket`(60)/`TCPSocket`(78) rows into
    // these are resolved after every class exists.
    BuiltinClass {
        id: BASIC_SOCKET_CLASS,
        name: "BasicSocket",
        is_module: false,
        superclass: Some(IO_CLASS),
        includes: &[],
        feature: Some("socket"),
    },
    BuiltinClass {
        id: IP_SOCKET_CLASS,
        name: "IPSocket",
        is_module: false,
        superclass: Some(BASIC_SOCKET_CLASS),
        includes: &[],
        feature: Some("socket"),
    },
    BuiltinClass {
        id: UDP_SOCKET_CLASS,
        name: "UDPSocket",
        is_module: false,
        superclass: Some(IP_SOCKET_CLASS),
        includes: &[],
        feature: Some("socket"),
    },
    BuiltinClass {
        id: UNIX_SOCKET_CLASS,
        name: "UNIXSocket",
        is_module: false,
        superclass: Some(BASIC_SOCKET_CLASS),
        includes: &[],
        feature: Some("socket"),
    },
    BuiltinClass {
        id: UNIX_SERVER_CLASS,
        name: "UNIXServer",
        is_module: false,
        superclass: Some(UNIX_SOCKET_CLASS),
        includes: &[],
        feature: Some("socket"),
    },
    BuiltinClass {
        id: ADDRINFO_CLASS,
        name: "Addrinfo",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("socket"),
    },
    // Ungated: `caller_locations` is core Kernel, available with no require.
    BuiltinClass {
        id: BACKTRACE_LOCATION_CLASS,
        name: "Thread::Backtrace::Location",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: FCNTL_MODULE,
        name: "Fcntl",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("fcntl"),
    },
    // Ungated: `io/console`'s methods are unconditional rows on the IO table
    // (see `docs/EXTENSIONS.md`), so the mode object they hand back has to
    // resolve without a require too.
    BuiltinClass {
        id: CONSOLE_MODE_CLASS,
        name: "IO::ConsoleMode",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: ZLIB_ZSTREAM_CLASS,
        name: "Zlib::ZStream",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("zlib"),
    },
    BuiltinClass {
        id: ZLIB_DEFLATE_CLASS,
        name: "Zlib::Deflate",
        is_module: false,
        superclass: Some(ZLIB_ZSTREAM_CLASS),
        includes: &[],
        feature: Some("zlib"),
    },
    BuiltinClass {
        id: ZLIB_INFLATE_CLASS,
        name: "Zlib::Inflate",
        is_module: false,
        superclass: Some(ZLIB_ZSTREAM_CLASS),
        includes: &[],
        feature: Some("zlib"),
    },
    BuiltinClass {
        id: ZLIB_GZIP_FILE_CLASS,
        name: "Zlib::GzipFile",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("zlib"),
    },
    BuiltinClass {
        id: ZLIB_GZIP_WRITER_CLASS,
        name: "Zlib::GzipWriter",
        is_module: false,
        superclass: Some(ZLIB_GZIP_FILE_CLASS),
        includes: &[],
        feature: Some("zlib"),
    },
    BuiltinClass {
        id: ZLIB_GZIP_READER_CLASS,
        name: "Zlib::GzipReader",
        is_module: false,
        superclass: Some(ZLIB_GZIP_FILE_CLASS),
        includes: &[ENUMERABLE_CLASS],
        feature: Some("zlib"),
    },
    BuiltinClass {
        id: PTY_MODULE,
        name: "PTY",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("pty"),
    },
    BuiltinClass {
        id: SYSLOG_MODULE,
        name: "Syslog",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("syslog"),
    },
    BuiltinClass {
        id: READLINE_MODULE,
        name: "Readline",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("readline"),
    },
    BuiltinClass {
        id: READLINE_HISTORY_CLASS,
        name: "Readline::History",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[ENUMERABLE_CLASS],
        feature: Some("readline"),
    },
    BuiltinClass {
        id: NKF_MODULE,
        name: "NKF",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("nkf"),
    },
    BuiltinClass {
        id: BIGDECIMAL_CLASS,
        name: "BigDecimal",
        is_module: false,
        superclass: Some(NUMERIC_CLASS),
        includes: &[],
        feature: Some("bigdecimal"),
    },
    // The ffi gem's runtime tier (dlopen + libffi calls), added for fiddle's
    // pure-Ruby FFI backend. `Builtin < Type` nests under it, so the `Type`
    // row precedes it. `Function < Pointer` and the constant-only
    // `AbstractMemory`/`AutoPointer` complete the gem's `is_a?` lattice.
    BuiltinClass {
        id: FFI_TYPE_CLASS,
        name: "FFI::Type",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("ffi"),
    },
    BuiltinClass {
        id: FFI_TYPE_BUILTIN_CLASS,
        name: "FFI::Type::Builtin",
        is_module: false,
        superclass: Some(FFI_TYPE_CLASS),
        includes: &[],
        feature: Some("ffi"),
    },
    BuiltinClass {
        id: FFI_DYNAMIC_LIBRARY_CLASS,
        name: "FFI::DynamicLibrary",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("ffi"),
    },
    BuiltinClass {
        id: FFI_FUNCTION_CLASS,
        name: "FFI::Function",
        is_module: false,
        superclass: Some(FFI_POINTER_CLASS),
        includes: &[],
        feature: Some("ffi"),
    },
    BuiltinClass {
        id: FFI_VARIADIC_INVOKER_CLASS,
        name: "FFI::VariadicInvoker",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("ffi"),
    },
    BuiltinClass {
        id: FFI_ABSTRACT_MEMORY_CLASS,
        name: "FFI::AbstractMemory",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("ffi"),
    },
    BuiltinClass {
        id: FFI_AUTO_POINTER_CLASS,
        name: "FFI::AutoPointer",
        is_module: false,
        superclass: Some(FFI_POINTER_CLASS),
        includes: &[],
        feature: Some("ffi"),
    },
    BuiltinClass {
        id: COVERAGE_MODULE,
        name: "Coverage",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("coverage"),
    },
    BuiltinClass {
        id: TRACEPOINT_CLASS,
        name: "TracePoint",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: OPENSSL_DIGEST_CLASS,
        name: "OpenSSL::Digest",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_DIGEST_MD4_CLASS,
        name: "OpenSSL::Digest::MD4",
        is_module: false,
        superclass: Some(OPENSSL_DIGEST_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_DIGEST_MD5_CLASS,
        name: "OpenSSL::Digest::MD5",
        is_module: false,
        superclass: Some(OPENSSL_DIGEST_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_DIGEST_RIPEMD160_CLASS,
        name: "OpenSSL::Digest::RIPEMD160",
        is_module: false,
        superclass: Some(OPENSSL_DIGEST_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_DIGEST_SHA1_CLASS,
        name: "OpenSSL::Digest::SHA1",
        is_module: false,
        superclass: Some(OPENSSL_DIGEST_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_DIGEST_SHA224_CLASS,
        name: "OpenSSL::Digest::SHA224",
        is_module: false,
        superclass: Some(OPENSSL_DIGEST_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_DIGEST_SHA256_CLASS,
        name: "OpenSSL::Digest::SHA256",
        is_module: false,
        superclass: Some(OPENSSL_DIGEST_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_DIGEST_SHA384_CLASS,
        name: "OpenSSL::Digest::SHA384",
        is_module: false,
        superclass: Some(OPENSSL_DIGEST_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_DIGEST_SHA512_CLASS,
        name: "OpenSSL::Digest::SHA512",
        is_module: false,
        superclass: Some(OPENSSL_DIGEST_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_HMAC_CLASS,
        name: "OpenSSL::HMAC",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_KDF_MODULE,
        name: "OpenSSL::KDF",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_BN_CLASS,
        name: "OpenSSL::BN",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_CIPHER_CLASS,
        name: "OpenSSL::Cipher",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_SSL_MODULE,
        name: "OpenSSL::SSL",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_SSL_CONTEXT_CLASS,
        name: "OpenSSL::SSL::SSLContext",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_SSL_SOCKET_CLASS,
        name: "OpenSSL::SSL::SSLSocket",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        // Upstream's ssl.rb writes `include Buffering` then `include
        // SocketForwarder`, which puts SocketForwarder FIRST in `ancestors`.
        includes: &[OPENSSL_BUFFERING_MODULE, OPENSSL_SOCKET_FORWARDER_MODULE],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_X509_MODULE,
        name: "OpenSSL::X509",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_X509_STORE_CLASS,
        name: "OpenSSL::X509::Store",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_X509_CERT_CLASS,
        name: "OpenSSL::X509::Certificate",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_X509_NAME_CLASS,
        name: "OpenSSL::X509::Name",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_RANDOM_MODULE,
        name: "OpenSSL::Random",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: BINDING_CLASS,
        name: "Binding",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: SOCKET_OPTION_CLASS,
        name: "Socket::Option",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("socket"),
    },
    BuiltinClass {
        id: OPENSSL_BUFFERING_MODULE,
        name: "OpenSSL::Buffering",
        is_module: true,
        superclass: None,
        includes: &[ENUMERABLE_CLASS],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_SOCKET_FORWARDER_MODULE,
        name: "OpenSSL::SSL::SocketForwarder",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: REFINEMENT_CLASS,
        name: "Refinement",
        is_module: false,
        superclass: Some(MODULE_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: PRISM_MODULE,
        name: "Prism",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("prism"),
    },
    BuiltinClass {
        id: FILE_CONSTANTS_MODULE,
        name: "File::Constants",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: ENUMERATOR_ARITHMETIC_SEQUENCE_CLASS,
        name: "Enumerator::ArithmeticSequence",
        is_module: false,
        superclass: Some(ENUMERATOR_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: PROCESS_SYS_MODULE,
        name: "Process::Sys",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: PROCESS_UID_MODULE,
        name: "Process::UID",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: PROCESS_GID_MODULE,
        name: "Process::GID",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
];

/// Top-level constant aliases for nested builtins Ruby ALSO exposes at the
/// top level: `::Queue = Thread::Queue`, `::Mutex = Thread::Mutex`, etc. The
/// class is defined `Thread::`-nested (so `.name`/`inspect` report the
/// qualified path, matching CRuby), while these aliases let bare `Queue`/
/// `Mutex`/`SizedQueue`/`ConditionVariable` still resolve at the top level.
/// `(alias_name, target_id)`; consulted last in name resolution so a user's
/// own top-level constant of the same name still wins.
pub const TOP_LEVEL_ALIASES: &[(&str, ClassId)] = &[
    ("Queue", QUEUE_CLASS),
    ("SizedQueue", SIZED_QUEUE_CLASS),
    ("Mutex", MUTEX_CLASS),
    ("ConditionVariable", CONDITION_VARIABLE_CLASS),
];

/// `Object`'s own hierarchy slot (it isn't a [`BUILTINS`] row):
/// superclass `BasicObject`, includes `Kernel` -- oracle-verified
/// `Object.ancestors == [Object, Kernel, BasicObject]`.
pub const OBJECT_SUPERCLASS: ClassId = BASIC_OBJECT_CLASS;
pub const OBJECT_INCLUDES: &[ClassId] = &[KERNEL_CLASS];

/// The first id the built-in exception classes occupy -- immediately after the
/// last [`BUILTINS`] row. DERIVED from the builtin count (`Object` is id 0 and
/// the builtins are `1..=BUILTINS.len()`, so the first free id is `len + 1`),
/// which is exactly where the compiler's sequential id counter lands after it
/// registers `Object` + every builtin. This is the whole point: **appending a
/// builtin automatically shifts the entire exception block** -- no hand-edited
/// ids, no re-learning the layout. Every [`EXCEPTION_CLASSES`] id is expressed
/// as [`exc_id`]`(offset)` off this base, so they all move together for free.
pub const FIRST_EXCEPTION_ID: u32 = BUILTINS.len() as u32 + 1;

/// The id of the exception class at position `offset` in [`EXCEPTION_CLASSES`]
/// -- the base plus its table index. Used for both the `id` and every
/// `superclass` edge so the whole block is relocatable by construction.
pub const fn exc_id(offset: u32) -> ClassId {
    ClassId(FIRST_EXCEPTION_ID + offset)
}

/// `Exception`, the root of the whole hierarchy (offset 0).
pub const EXCEPTION_CLASS: ClassId = exc_id(0);

/// `StopIteration` -- named because the runtime special-cases it (an
/// `each`-driver's terminal signal). Derived like every other exception id, so
/// it never needs a manual bump. Keep the offset in sync with its row.
pub const STOP_ITERATION_CLASS: ClassId = exc_id(15);

/// `KeyError` -- named because it exposes the typed accessors `#key`/`#receiver`
/// over an exception's hidden detail slots. Keep the offset in sync with its row.
pub const KEY_ERROR_CLASS: ClassId = exc_id(14);

/// `NameError` -- exposes `#name`/`#receiver` and a name-aware `initialize`.
pub const NAME_ERROR_CLASS: ClassId = exc_id(16);

/// `NoMethodError` (a `NameError`) -- additionally exposes `#args`.
pub const NO_METHOD_ERROR_CLASS: ClassId = exc_id(17);

/// `UncaughtThrowError` -- exposes `#tag`/`#value` from an uncaught `throw`.
pub const UNCAUGHT_THROW_ERROR_CLASS: ClassId = exc_id(35);

/// `SystemCallError` -- the parent every [`ERRNO_CLASSES`] row gets, and the
/// class an unmapped errno falls back to.
pub const SYSTEM_CALL_ERROR_CLASS: ClassId = exc_id(31);

/// The `Errno` namespace module, which owns every [`ERRNO_CLASSES`] name and
/// every [`ERRNO_ALIASES`] constant.
pub const ERRNO_MODULE: ClassId = exc_id(32);

/// `LocalJumpError` -- exposes `#reason`/`#exit_value`.
/// `SyntaxError` -- carries `#path`, the file whose parse failed.
pub const SYNTAX_ERROR_CLASS: ClassId = exc_id(34);
/// `NoMatchingPatternKeyError` -- carries `#key` and `#matchee`, the Hash key
/// a `=>`/`in` pattern asked for and the Hash it asked of.
pub const NO_MATCHING_PATTERN_KEY_ERROR_CLASS: ClassId = exc_id(42);

/// `Encoding::UndefinedConversionError` -- a valid source character with no
/// representation in the target. Carries `#error_char` and the encoding pair.
pub const UNDEFINED_CONVERSION_ERROR_CLASS: ClassId = exc_id(7);
/// `Encoding::InvalidByteSequenceError` -- bytes the source encoding cannot
/// decode. Carries `#error_bytes`/`#readagain_bytes` and the encoding pair.
pub const INVALID_BYTE_SEQUENCE_ERROR_CLASS: ClassId = exc_id(8);

pub const LOCAL_JUMP_ERROR_CLASS: ClassId = exc_id(20);

/// `FrozenError` -- exposes `#receiver` (the frozen object).
pub const FROZEN_ERROR_CLASS: ClassId = exc_id(23);

/// `LoadError` -- exposes `#path` (the feature that would not load).
pub const LOAD_ERROR_CLASS: ClassId = exc_id(3);

/// `SystemExit` -- carries an exit status via `#status`/`#success?`.
pub const SYSTEM_EXIT_CLASS: ClassId = exc_id(36);

/// `SignalException` -- resolves a signal name/number in `initialize` and
/// exposes `#signo`/`#signm`.
pub const SIGNAL_EXCEPTION_CLASS: ClassId = exc_id(37);

/// `Interrupt` (a `SignalException`) -- fixed to `SIGINT` (signo 2).
pub const INTERRUPT_CLASS: ClassId = exc_id(38);

/// One row of the built-in exception hierarchy -- the shared source of truth
/// for the ids both sides bake in.
#[derive(Clone, Copy)]
pub struct ExceptionClass {
    pub id: ClassId,
    /// Fully-qualified Ruby name (`"ArgumentError"`, `"Encoding::CompatibilityError"`).
    pub name: &'static str,
    /// Superclass id. `None` only for the `Errno` MODULE (a namespace, not a
    /// class); every real exception class has one, up to `Exception`, whose
    /// superclass is `Object`.
    pub superclass: Option<ClassId>,
    /// `true` for the `Errno` namespace module.
    pub is_module: bool,
}

/// The built-in exception hierarchy, in the exact order the compiler registers
/// it: the [`BUILTIN_EXCEPTIONS_RB`](../zeo/parse) Ruby source, the pinned tail
/// that starts at `Math::DomainError`, the whole [`ERRNO_CLASSES`] block, and
/// finally the readiness rows that subclass an `Errno` class. Ids are
/// contiguous from [`FIRST_EXCEPTION_ID`] (asserted below), so row `i` has id
/// [`exc_id`]`(i)` -- both the `id` and every `superclass` edge are written
/// that way, so appending a builtin (which bumps [`FIRST_EXCEPTION_ID`])
/// relocates the whole block automatically. This is what lets `zeo-rt`'s
/// `register_exceptions` install these classes at ids the compiler
/// independently assigns the same way -- `zeo` asserts the agreement at
/// analyze time.
///
/// Superclass edges point earlier in the table, except for the last six rows,
/// whose parents live in the `Errno` block ahead of them -- the block's length
/// is a platform fact, so it cannot be spelled as a literal id and the rows
/// that need it must follow it. Nothing reads the table in order:
/// [`declared_ancestors`] resolves each edge by id and linearizes up to
/// `Object`.
pub const EXCEPTION_CLASSES: &[ExceptionClass] = &EXCEPTION_CLASS_ROWS;

/// The rows written out by hand: every exception whose id does not depend on
/// how many errnos the platform names. [`ERRNO_CLASSES`] follows this block,
/// then [`WAIT_EXCEPTIONS`], which subclasses two of the `Errno` rows.
const CORE_EXCEPTIONS: [ExceptionClass; 46] = [
    ExceptionClass {
        id: exc_id(0),
        name: "Exception",
        superclass: Some(OBJECT_CLASS),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(1),
        name: "ScriptError",
        superclass: Some(exc_id(0)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(2),
        name: "NotImplementedError",
        superclass: Some(exc_id(1)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(3),
        name: "LoadError",
        superclass: Some(exc_id(1)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(4),
        name: "StandardError",
        superclass: Some(exc_id(0)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(5),
        name: "ArgumentError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(6),
        name: "EncodingError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(7),
        name: "Encoding::UndefinedConversionError",
        superclass: Some(exc_id(6)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(8),
        name: "Encoding::InvalidByteSequenceError",
        superclass: Some(exc_id(6)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(9),
        name: "Encoding::CompatibilityError",
        superclass: Some(exc_id(6)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(10),
        name: "Encoding::ConverterNotFoundError",
        superclass: Some(exc_id(6)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(11),
        name: "IOError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(12),
        name: "EOFError",
        superclass: Some(exc_id(11)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(13),
        name: "IndexError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(14),
        name: "KeyError",
        superclass: Some(exc_id(13)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(15),
        name: "StopIteration",
        superclass: Some(exc_id(13)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(16),
        name: "NameError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(17),
        name: "NoMethodError",
        superclass: Some(exc_id(16)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(18),
        name: "RangeError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(19),
        name: "FloatDomainError",
        superclass: Some(exc_id(18)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(20),
        name: "LocalJumpError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(21),
        name: "RegexpError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(22),
        name: "RuntimeError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(23),
        name: "FrozenError",
        superclass: Some(exc_id(22)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(24),
        name: "NoMatchingPatternError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(25),
        name: "FiberError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(26),
        name: "ThreadError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(27),
        name: "ClosedQueueError",
        superclass: Some(exc_id(15)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(28),
        name: "RactorError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(29),
        name: "TypeError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(30),
        name: "ZeroDivisionError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(31),
        name: "SystemCallError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(32),
        name: "Errno",
        superclass: None,
        is_module: true,
    },
    ExceptionClass {
        id: exc_id(33),
        name: "Math::DomainError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    // `SyntaxError < ScriptError` -- raised by the runtime eval VM
    // when a dynamically-eval'd string fails to parse. Appended AFTER
    // `Math::DomainError` so every pre-existing exception id stays put; like
    // `Math::DomainError` it is registered in the compiler's exception-tail pin
    // rather than in `BUILTIN_EXCEPTIONS_RB` (id-ordering, not a semantic
    // difference).
    ExceptionClass {
        id: exc_id(34),
        name: "SyntaxError",
        superclass: Some(exc_id(1)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(35),
        name: "UncaughtThrowError",
        superclass: Some(exc_id(5)),
        is_module: false,
    },
    // The non-`StandardError` exception tail: a bare `rescue` never catches
    // these (they descend from `Exception` directly), so a program must name
    // them explicitly. `Interrupt < SignalException` mirrors CRuby's SIGINT
    // class. Pinned here (see `analyze::pin_builtin_exceptions_tail`).
    ExceptionClass {
        id: exc_id(36),
        name: "SystemExit",
        superclass: Some(exc_id(0)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(37),
        name: "SignalException",
        superclass: Some(exc_id(0)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(38),
        name: "Interrupt",
        superclass: Some(exc_id(37)),
        is_module: false,
    },
    // The remaining core `Exception`-tree classes CRuby defines (gem- and
    // Ractor-specific ones excluded). `NoMemoryError`/`SecurityError`/
    // `SystemStackError` descend from `Exception` directly (uncaught by a bare
    // `rescue`); the rest refine an existing `StandardError` branch.
    ExceptionClass {
        id: exc_id(39),
        name: "NoMemoryError",
        superclass: Some(exc_id(0)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(40),
        name: "SecurityError",
        superclass: Some(exc_id(0)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(41),
        name: "SystemStackError",
        superclass: Some(exc_id(0)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(42),
        name: "NoMatchingPatternKeyError",
        superclass: Some(exc_id(24)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(43),
        name: "Regexp::TimeoutError",
        superclass: Some(exc_id(21)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(44),
        name: "IO::TimeoutError",
        superclass: Some(exc_id(11)),
        is_module: false,
    },
    // `WeakRef::RefError` -- raised by a `WeakRef` whose referent has been
    // collected. CRuby makes it a plain `StandardError` (exc_id(4)).
    ExceptionClass {
        id: exc_id(45),
        name: "WeakRef::RefError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
];

/// How many rows precede the `Errno` block.
const CORE_EXCEPTION_COUNT: usize = CORE_EXCEPTIONS.len();

/// The id of the `Errno` class called `name`. The block's position depends on
/// the platform's errno set, so this is the only way to name one; it fails the
/// build for a name no [`ERRNO_CLASSES`] row carries.
pub const fn errno_class_id(name: &str) -> ClassId {
    let mut i = 0;
    while i < ERRNO_CLASSES.len() {
        if const_str_eq(ERRNO_CLASSES[i].name, name) {
            return exc_id((CORE_EXCEPTION_COUNT + i) as u32);
        }
        i += 1;
    }
    panic!("no such Errno class");
}

/// `str::eq` is not callable in a const fn, and this crate takes no
/// dependencies to borrow one from.
const fn const_str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// The errno `class` stands for, or `None` when it is not an `Errno` class at
/// all. The block is contiguous, so this is a range check, not a search.
pub fn errno_of_class(class: ClassId) -> Option<i32> {
    let first = exc_id(CORE_EXCEPTION_COUNT as u32).0;
    let offset = class.0.checked_sub(first)? as usize;
    ERRNO_CLASSES.get(offset).map(|row| row.errno)
}

/// The `Errno` class and value for `errno`, or `None` when this platform names
/// no class for it -- an errno CRuby would leave as a bare `SystemCallError`.
pub fn errno_class(errno: i32) -> Option<(ClassId, &'static ErrnoClass)> {
    // Zero is `Errno::NOERROR`, a class no failure ever carries: an `errno` of
    // zero means the call SUCCEEDED, so a lookup for it is a caller bug rather
    // than the "no such class" the `None` arm reports.
    ERRNO_CLASSES
        .iter()
        .position(|row| row.errno == errno && errno != 0)
        .map(|i| (exc_id((CORE_EXCEPTION_COUNT + i) as u32), &ERRNO_CLASSES[i]))
}

/// The id of the readiness row at `offset`, which follows the whole `Errno`
/// block.
const fn wait_id(offset: usize) -> ClassId {
    exc_id((CORE_EXCEPTION_COUNT + ERRNO_CLASSES.len() + offset) as u32)
}

/// The non-blocking readiness rows, last because two of them subclass an
/// `Errno` class. `IO::WaitReadable`/`WaitWritable` are marker MODULES, so a
/// would-block errno can be rescued by protocol rather than by errno.
const WAIT_EXCEPTIONS: [ExceptionClass; 6] = [
    ExceptionClass {
        id: wait_id(0),
        name: "IO::WaitReadable",
        superclass: None,
        is_module: true,
    },
    ExceptionClass {
        id: wait_id(1),
        name: "IO::WaitWritable",
        superclass: None,
        is_module: true,
    },
    ExceptionClass {
        id: wait_id(2),
        name: "IO::EAGAINWaitReadable",
        superclass: Some(errno_class_id("Errno::EAGAIN")),
        is_module: false,
    },
    ExceptionClass {
        id: wait_id(3),
        name: "IO::EAGAINWaitWritable",
        superclass: Some(errno_class_id("Errno::EAGAIN")),
        is_module: false,
    },
    ExceptionClass {
        id: wait_id(4),
        name: "IO::EINPROGRESSWaitReadable",
        superclass: Some(errno_class_id("Errno::EINPROGRESS")),
        is_module: false,
    },
    ExceptionClass {
        id: wait_id(5),
        name: "IO::EINPROGRESSWaitWritable",
        superclass: Some(errno_class_id("Errno::EINPROGRESS")),
        is_module: false,
    },
];

const EXCEPTION_CLASS_COUNT: usize =
    CORE_EXCEPTION_COUNT + ERRNO_CLASSES.len() + WAIT_EXCEPTIONS.len();

const EXCEPTION_CLASS_ROWS: [ExceptionClass; EXCEPTION_CLASS_COUNT] = build_exception_classes();

/// Splice the three blocks into one contiguously-numbered table. Every `Errno`
/// class is a bare `SystemCallError` subclass, so the middle block needs no
/// per-row source of its own -- [`ERRNO_CLASSES`] carries all of it.
const fn build_exception_classes() -> [ExceptionClass; EXCEPTION_CLASS_COUNT] {
    let mut out = [CORE_EXCEPTIONS[0]; EXCEPTION_CLASS_COUNT];
    let mut i = 0;
    while i < CORE_EXCEPTION_COUNT {
        out[i] = CORE_EXCEPTIONS[i];
        i += 1;
    }
    let mut j = 0;
    while j < ERRNO_CLASSES.len() {
        out[CORE_EXCEPTION_COUNT + j] = ExceptionClass {
            id: exc_id((CORE_EXCEPTION_COUNT + j) as u32),
            name: ERRNO_CLASSES[j].name,
            superclass: Some(SYSTEM_CALL_ERROR_CLASS),
            is_module: false,
        };
        j += 1;
    }
    let mut k = 0;
    while k < WAIT_EXCEPTIONS.len() {
        out[CORE_EXCEPTION_COUNT + ERRNO_CLASSES.len() + k] = WAIT_EXCEPTIONS[k];
        k += 1;
    }
    out
}

/// The exception rows that INCLUDE a module. Only the non-blocking readiness
/// classes do, so a side table beats an `includes` field every other row would
/// have to spell as empty.
const EXCEPTION_INCLUDES: &[(ClassId, &[ClassId])] = &[
    (wait_id(2), &[wait_id(0)]),
    (wait_id(3), &[wait_id(1)]),
    (wait_id(4), &[wait_id(0)]),
    (wait_id(5), &[wait_id(1)]),
];

/// A core class's `(superclass, includes)` edges, covering `Object`, every
/// [`BUILTINS`] row, and every [`EXCEPTION_CLASSES`] row. The single source both
/// the compiler's seeding (`Compiler::new`) and the runtime's registry
/// (`ClassRegistry::with_core`) derive the hierarchy from. Almost no exception
/// includes a module; the few that do are in [`EXCEPTION_INCLUDES`].
fn core_class_edges(id: ClassId) -> (Option<ClassId>, &'static [ClassId]) {
    if id == OBJECT_CLASS {
        return (Some(OBJECT_SUPERCLASS), OBJECT_INCLUDES);
    }
    if let Some(b) = BUILTINS.iter().find(|b| b.id == id) {
        return (b.superclass, b.includes);
    }
    if let Some(e) = EXCEPTION_CLASSES.iter().find(|e| e.id == id) {
        let includes = EXCEPTION_INCLUDES
            .iter()
            .find(|(c, _)| *c == id)
            .map_or(&[][..], |(_, m)| *m);
        return (e.superclass, includes);
    }
    (None, &[])
}

fn expand_core(id: ClassId, out: &mut Vec<ClassId>) {
    if out.contains(&id) {
        return;
    }
    out.push(id);
    let (superclass, includes) = core_class_edges(id);
    // `includes` reversed, then the superclass -- the exact order (and the
    // dedup above) the compiler's `mro::expand_into` uses, so an UNMODIFIED
    // core class linearizes here identically to how the compiler linearizes it.
    for &m in includes.iter().rev() {
        expand_core(m, out);
    }
    if let Some(parent) = superclass {
        expand_core(parent, out);
    }
}

/// A core class's DECLARED linearized ancestors -- what the abi declares for it
/// before any program reopens it. Covers builtins AND exceptions with one DFS:
/// e.g. `declared_ancestors(StandardError)` walks its superclass chain up to
/// `Exception`, then `Object`'s own tail, giving
/// `[StandardError, Exception, Object, Kernel, BasicObject]`.
///
/// The runtime installs these once (`ClassRegistry::with_core`); the compiler
/// emits a per-program OVERRIDE only when a program actually changes a builtin's
/// ancestors (so `class Array; include M; end` still works, full-parity, without
/// every program re-listing the unchanged hierarchy).
/// Whether this builtin needs a per-program registry entry: `register_builtins`
/// installs exactly the UNGATED ones, so a `require`-gated class must be
/// registered by the program that activates it.
///
/// Asked of the ABI rather than of the compiler's own `feature_gate`, which a
/// reopen deliberately clears to materialize the constant -- that changes name
/// resolution, never who registered the class.
pub fn is_gated_builtin(id: ClassId) -> bool {
    BUILTINS
        .iter()
        .any(|b| b.id.0 == id.0 && b.feature.is_some())
}

pub fn declared_ancestors(id: ClassId) -> Vec<ClassId> {
    let mut out = Vec::new();
    expand_core(id, &mut out);
    out
}

/// The Ruby-visible name of any builtin id, `Object` included. `None` for
/// user-class ids. Backs NoMethodError messages and registry-less display.
pub fn builtin_name(id: ClassId) -> Option<&'static str> {
    if id == OBJECT_CLASS {
        return Some("Object");
    }
    BUILTINS
        .get((id.0 as usize).wrapping_sub(1))
        .filter(|b| b.id == id)
        .map(|b| b.name)
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
];

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
