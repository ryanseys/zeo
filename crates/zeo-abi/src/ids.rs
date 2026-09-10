//! One `ClassId` constant per built-in class and module.
//!
//! The numbers are APPEND-ONLY: renumbering is safe (nothing persists
//! across builds), but appending keeps generated-code diffs reviewable.

use super::*;

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
/// `Digest::SHA384`, and `Digest::SHA2` -- the BIT-LENGTH-parameterized
/// class, which is a real class of its own rather than an alias for one of
/// the fixed-width rows.
/// (These two sit at 107/108 because `BUILTINS` must stay contiguous and
/// those ids are free.)
pub const DIGEST_SHA384_CLASS: ClassId = ClassId(107);
pub const DIGEST_SHA2_CLASS: ClassId = ClassId(108);
/// The `digest` framework's ancestry, exactly ruby's: every fixed-width
/// algorithm class `< Digest::Base < Digest::Class`, `SHA2 < Digest::Class`
/// directly, and `Digest::Class` includes `Digest::Instance`. The gem's
/// vendored Ruby half reopens `Class` and `Instance` with the file and
/// base64digest families.
pub const DIGEST_INSTANCE_MODULE: ClassId = ClassId(176);
pub const DIGEST_CLASS_CLASS: ClassId = ClassId(177);
pub const DIGEST_BASE_CLASS: ClassId = ClassId(178);

/// `Zeo` -- the runtime's own namespace -- and `Zeo::Eval`, the async
/// snippet-compile surface (`prepare`). Runtime-provided, no feature
/// gate; the 14 MB compiler still links only when `uses_runtime_eval`
/// sees a site.
pub const ZEO_MODULE: ClassId = ClassId(179);
pub const ZEO_EVAL_MODULE: ClassId = ClassId(180);
/// `json`: the `JSON` module (parser/generator). Behind `ext-json`.
pub const JSON_MODULE: ClassId = ClassId(55);
/// `date`: `Date`/`DateTime`. Behind `ext-date`.
pub const DATE_CLASS: ClassId = ClassId(56);
pub const DATETIME_CLASS: ClassId = ClassId(57);
/// `zlib`: the `Zlib` compression module. Behind `ext-zlib`.
pub const ZLIB_MODULE: ClassId = ClassId(58);
/// `psych`: the `Psych` YAML module. Behind `ext-psych`.
pub const PSYCH_MODULE: ClassId = ClassId(59);
/// `socket`: the `Socket` class. Behind `ext-socket`.
pub const SOCKET_CLASS: ClassId = ClassId(60);
/// `openssl`: the `OpenSSL` module. Behind `ext-openssl`.
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
/// The builtin slots that are NAMESPACES only -- the class itself is defined in
/// Ruby (a vendored gem), and the row exists so a nested builtin constant has a
/// scope and so [`BUILTINS`] stays contiguous. A reopen may therefore DECLARE
/// the superclass the row leaves open, which is a mismatch for every other
/// builtin.
pub const NAMESPACE_PLACEHOLDERS: &[ClassId] = &[WEAKREF_CLASS];

/// Whether `id` is one of [`NAMESPACE_PLACEHOLDERS`].
pub fn is_namespace_placeholder(id: ClassId) -> bool {
    NAMESPACE_PLACEHOLDERS.contains(&id)
}

/// `WeakRef`'s NAMESPACE slot. The class itself is the vendored pure-Ruby
/// the `weakref` gem -- CRuby's own file, `class WeakRef < Delegator`, standing
/// on the `ObjectSpace::WeakMap` beside it. The builtin row carries no
/// methods; it exists so `WeakRef::RefError` has a scope to nest under.
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

/// `UnicodeNormalize` -- an EMPTY module in ruby: it declares no method and
/// no constant of its own, and exists as the namespace `String`'s
/// `#unicode_normalize` family is documented under.
pub const UNICODE_NORMALIZE_MODULE: ClassId = ClassId(159);

/// `Set::CoreSet` -- a `Set` subclass with no methods of its own, which ruby
/// 4.0 keeps as the name for the core implementation.
pub const SET_CORE_SET_CLASS: ClassId = ClassId(160);

/// `Process::Waiter` -- the `Thread` subclass `Process.detach` answers: a
/// real thread whose class pointer CRuby retags after creation, with `#pid`
/// as its one own method.
pub const PROCESS_WAITER_CLASS: ClassId = ClassId(161);

/// `Ractor::Port` -- ruby 4.0's message endpoint. Every Ractor owns a default
/// port plus any `Ractor::Port.new` ports its own thread creates; any ractor
/// may `#send` to a port, only the creator may `#receive`/`#close`.
pub const RACTOR_PORT_CLASS: ClassId = ClassId(162);

/// `Ractor::MovedObject` -- the `BasicObject` husk left behind by `move: true`
/// sends. Every method on it raises `Ractor::MovedError`.
pub const RACTOR_MOVED_OBJECT_CLASS: ClassId = ClassId(163);

/// `IO::Buffer` -- fixed-size byte storage with typed value access
/// (`get_value(:U32, 0)`), slicing, file mapping, and direct IO transfer.
/// Includes `Comparable` (byte-lexicographic `<=>`).
pub const IO_BUFFER_CLASS: ClassId = ClassId(164);

/// `RubyVM` -- CRuby's VM introspection namespace, emulated over zeo's own
/// counters and the prism parser.
pub const RUBYVM_CLASS: ClassId = ClassId(165);
/// `RubyVM::AbstractSyntaxTree` -- parse.y-taxonomy AST over prism.
pub const RUBYVM_AST_MODULE: ClassId = ClassId(166);
/// `RubyVM::AbstractSyntaxTree::Node`.
pub const RUBYVM_AST_NODE_CLASS: ClassId = ClassId(167);
/// `RubyVM::AbstractSyntaxTree::Location`.
pub const RUBYVM_AST_LOCATION_CLASS: ClassId = ClassId(168);
/// `RubyVM::InstructionSequence` -- compile/eval work; serialization
/// truthfully refuses (zeo has no YARV).
pub const RUBYVM_ISEQ_CLASS: ClassId = ClassId(169);
/// `RubyVM::YJIT` -- present, permanently disabled (zeo is AOT).
pub const RUBYVM_YJIT_MODULE: ClassId = ClassId(170);

/// The `Ruby` namespace module -- carries the `RUBY_*` identity constants
/// under their modern spellings, plus `Ruby::Box`.
pub const RUBY_MODULE: ClassId = ClassId(171);
/// `Ruby::Box < Module` -- namespace isolation, env-gated (`RUBY_BOX=1`).
pub const RUBY_BOX_CLASS: ClassId = ClassId(172);
/// `Ruby::Box::Entry`.
pub const RUBY_BOX_ENTRY_CLASS: ClassId = ClassId(173);
/// `Ruby::Box::Loader`.
pub const RUBY_BOX_LOADER_MODULE: ClassId = ClassId(174);

/// `FFI::Union < FFI::Struct` -- the same `layout` directive, every member at
/// offset 0. sassc declares its tagged value that way.
pub const FFI_UNION_CLASS: ClassId = ClassId(175);

/// `Socket::Constants` -- the address-family / socket-type / protocol names,
/// as a namespace of their own. CRuby's `sock_define_const` defines each name
/// TWICE, once here and once on `Socket` itself, so the two tables hold the
/// same values and neither is derived from the other. It is NOT included
/// anywhere -- `Socket.include?(Socket::Constants)` is false, and
/// `Socket::Constants.ancestors` is just itself -- so this row carries no
/// edges either. celluloid-io's `Constants = ::Socket::Constants` is the shape
/// that wanted it.
pub const SOCKET_CONSTANTS_MODULE: ClassId = ClassId(102);

/// `CGI::Escape` and `CGI::EscapeExt` -- where CRuby's `cgi/escape` actually
/// puts the escape helpers. `CGI.escapeHTML` is not a class method of `CGI`:
/// it is an instance method of one of these two, reached because `CGI` both
/// INCLUDES and EXTENDS `Escape`, and `Escape` PREPENDS `ExscapeExt`. All
/// four edges are observable and none is derivable from the others:
///
///   CGI.ancestors                  [CGI, EscapeExt, Escape, Object, ...]
///   CGI.singleton_class.ancestors  [#<Class:CGI>, EscapeExt, Escape, ...]
///   CGI::Escape.ancestors          [EscapeExt, Escape]
///   CGI.method(:escapeHTML).owner  CGI::EscapeExt
///   CGI.method(:escapeElement).owner  CGI::Escape
///
/// The two modules overlap on eight names and differ on the rest: only
/// `Escape` has the `*Element` family, only `EscapeExt` has `h` and the
/// `escape_html`/`unescape_html` snake spellings. erubi asks
/// `defined?(::CGI::Escape)` and net-http-persistent asks
/// `defined?(CGI::EscapeExt)`, so both names have to be real.
pub const CGI_ESCAPE_MODULE: ClassId = ClassId(103);
pub const CGI_ESCAPE_EXT_MODULE: ClassId = ClassId(104);

/// `Pathname` -- a path as a value. Reachable with NO `require`: ruby 4.0
/// loads `pathname.so` before the first line, so the class and 96 of its
/// methods are there whatever the program does.
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
/// no constructor, so they live in `ext/zlib/lib/zlib.rb` (see `ext/mod.rs`).
pub const ZLIB_ZSTREAM_CLASS: ClassId = ClassId(99);
/// `Zlib::Deflate < Zlib::ZStream` -- a compressor.
pub const ZLIB_DEFLATE_CLASS: ClassId = ClassId(100);
/// `Zlib::Inflate < Zlib::ZStream` -- a decompressor.
pub const ZLIB_INFLATE_CLASS: ClassId = ClassId(101);
// ClassIds 102-104 were the native `Zlib::GzipFile` family. The gzip
// container classes are plain Ruby now (`ext/zlib/lib/zlib.rb`) over the raw
// `Deflate`/`Inflate` streams, and `SOCKET_CONSTANTS_MODULE` and the two CGI
// escape modules hold these ids to keep the table contiguous.
/// `pty`: the `PTY` module -- pseudo-terminal allocation (`.open`) and
/// child processes run under one (`.spawn`/`.getpty`, `.check`). Its
/// `ChildExited` exception lives in the gem's Ruby half (`ext/pty`), the
/// same split as `Zlib`'s errors.
pub const PTY_MODULE: ClassId = ClassId(105);
/// `syslog`: the `Syslog` module over the system `syslog(3)` facility --
/// `open`/`log`/`mask` plus the priority/facility/option constant set. Its
/// `Constants`/`Level`/`Option`/`Facility`/`Macros` submodules live in the
/// gem's Ruby half (`ext/syslog`).
pub const SYSLOG_MODULE: ClassId = ClassId(106);
// ClassIds 107 and 108 were `Readline` and its history class. The native
// readline extension is deleted -- the official pure readline gem rides the
// lock and answers `Readline` as reline, ruby's own arrangement -- and the
// two Digest SHA classes above hold the ids to keep the table contiguous.
/// `nkf`: the `NKF` module -- Network Kanji Filter, Japanese text encoding
/// conversion (`.nkf` over an option string, `.guess`) rebuilt over the
/// runtime's own encoding engine. The `Kconv` wrapper is the gem's Ruby
/// half (`ext/nkf`).
pub const NKF_MODULE: ClassId = ClassId(109);
/// `bigdecimal`: the `BigDecimal` class -- arbitrary-precision decimal
/// arithmetic. The native half is bigdecimal 4.x's C slice (exact
/// arithmetic, division precision, rounding, mode state); `power`/`sqrt`/
/// `BigMath` and the `to_d` family are the gem's own Ruby, vendored in
/// `crates/zeo-rt/ext/bigdecimal`. Not constructible via `new` (CRuby removed it); the
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
/// `ext/openssl/lib` -- see `ext/mod.rs` on why a gated native class
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

/// `Thread::Backtrace` -- the namespace [`BACKTRACE_LOCATION_CLASS`] nests
/// under, plus the one class method (`.limit`) CRuby puts on it. Its id is
/// LATER than the location's, which the compiler's two-pass nesting handles.
pub const BACKTRACE_CLASS: ClassId = ClassId(152);

/// `GC::Profiler` -- the GC timing recorder's switch and readouts.
pub const GC_PROFILER_MODULE: ClassId = ClassId(153);

/// `ObjectSpace::WeakKeyMap` -- [`WEAKMAP_CLASS`]'s sibling, weak on the KEY
/// side only, so a value may safely reference its own key's map.
pub const WEAK_KEY_MAP_CLASS: ClassId = ClassId(154);

/// `Random::Base` -- the rung between `Random` and `Object` that actually
/// holds the generator (`#rand`/`#bytes`/`#seed` are ITS methods, not
/// `Random`'s), and where [`RANDOM_FORMATTER_MODULE`] mixes in.
pub const RANDOM_BASE_CLASS: ClassId = ClassId(155);

/// `Enumerator::Generator` and `Enumerator::Producer` -- what
/// `Enumerator.new { |y| ... }` and `Enumerator.produce` hold as their
/// source. Each answers `#each` and nothing else.
pub const ENUMERATOR_GENERATOR_CLASS: ClassId = ClassId(156);
pub const ENUMERATOR_PRODUCER_CLASS: ClassId = ClassId(157);

/// `Encoding::Converter` -- the stateful, chunk-at-a-time face of the same
/// engine `String#encode` runs on.
pub const ENCODING_CONVERTER_CLASS: ClassId = ClassId(158);

/// `Refinement` -- what `M.refinements` holds and what a refined method's
/// `Method#owner` reports. A `Module` subclass with no instances of its
/// own here: the compiler mints one hidden module per `refine` block and
/// MARKS it, which is what makes `owner.class` answer `Refinement` while
/// `Module` still answers for every ordinary module.
pub const REFINEMENT_CLASS: ClassId = ClassId(145);

/// `Socket::Option` -- one socket option's `(family, level, optname, data)`,
/// which `BasicSocket#getsockopt` answers.
pub const SOCKET_OPTION_CLASS: ClassId = ClassId(142);
