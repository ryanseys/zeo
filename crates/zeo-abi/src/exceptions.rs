//! Every built-in exception class: its id, its row, and the `Errno`
//! family generated from the platform's own list.

use super::*;

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

/// `NotImplementedError` -- named because the emitter raises it where zeo
/// declines a shape ruby answers, rather than answering it wrongly.
pub const NOT_IMPLEMENTED_ERROR_CLASS: ClassId = exc_id(2);

/// `NameError` -- exposes `#name`/`#receiver` and a name-aware `initialize`.
pub const NAME_ERROR_CLASS: ClassId = exc_id(16);

/// `NoMethodError` (a `NameError`) -- additionally exposes `#args`.
pub const NO_METHOD_ERROR_CLASS: ClassId = exc_id(17);

/// `ZeroDivisionError` -- named because the emitted integer `/`/`%` guards
/// raise it inline rather than through a dispatch.
pub const ZERO_DIVISION_ERROR_CLASS: ClassId = exc_id(29);

/// `UncaughtThrowError` -- exposes `#tag`/`#value` from an uncaught `throw`.
pub const UNCAUGHT_THROW_ERROR_CLASS: ClassId = exc_id(34);

/// `SystemCallError` -- the parent every [`ERRNO_CLASSES`] row gets, and the
/// class an unmapped errno falls back to.
pub const SYSTEM_CALL_ERROR_CLASS: ClassId = exc_id(30);

/// The `Errno` namespace module, which owns every [`ERRNO_CLASSES`] name and
/// every [`ERRNO_ALIASES`] constant.
pub const ERRNO_MODULE: ClassId = exc_id(31);

/// `SyntaxError` -- carries `#path`, the file whose parse failed.
pub const SYNTAX_ERROR_CLASS: ClassId = exc_id(33);
/// `NoMatchingPatternKeyError` -- carries `#key` and `#matchee`, the Hash key
/// a `=>`/`in` pattern asked for and the Hash it asked of.
pub const NO_MATCHING_PATTERN_KEY_ERROR_CLASS: ClassId = exc_id(41);
/// `Ractor::RemoteError` -- the one class in the `Ractor` error tree with a
/// method of its own (`#ractor`, the ractor whose failure it relays).
pub const RACTOR_REMOTE_ERROR_CLASS: ClassId = exc_id(49);

/// `Encoding::UndefinedConversionError` -- a valid source character with no
/// representation in the target. Carries `#error_char` and the encoding pair.
pub const UNDEFINED_CONVERSION_ERROR_CLASS: ClassId = exc_id(7);
/// `Encoding::InvalidByteSequenceError` -- bytes the source encoding cannot
/// decode. Carries `#error_bytes`/`#readagain_bytes` and the encoding pair.
pub const INVALID_BYTE_SEQUENCE_ERROR_CLASS: ClassId = exc_id(8);

/// `LocalJumpError` -- exposes `#reason`/`#exit_value`.
pub const LOCAL_JUMP_ERROR_CLASS: ClassId = exc_id(20);

/// CRuby's internal `fatal`, raised when no thread can make progress. It
/// descends straight from `Exception`, so `rescue => e` does not catch it.
pub const FATAL_CLASS: ClassId = exc_id(56);

/// `RuntimeError` -- the one class whose EMPTY message renders as
/// `unhandled exception` rather than the class name (`rb_decorate_message`).
pub const RUNTIME_ERROR_CLASS: ClassId = exc_id(22);

/// `FrozenError` -- exposes `#receiver` (the frozen object).
pub const FROZEN_ERROR_CLASS: ClassId = exc_id(23);

/// `LoadError` -- exposes `#path` (the feature that would not load).
pub const LOAD_ERROR_CLASS: ClassId = exc_id(3);

/// `SystemExit` -- carries an exit status via `#status`/`#success?`.
pub const SYSTEM_EXIT_CLASS: ClassId = exc_id(35);

/// `SignalException` -- resolves a signal name/number in `initialize` and
/// exposes `#signo`/`#signm`.
pub const SIGNAL_EXCEPTION_CLASS: ClassId = exc_id(36);

/// `Interrupt` (a `SignalException`) -- fixed to `SIGINT` (signo 2).
pub const INTERRUPT_CLASS: ClassId = exc_id(37);

// The ids `zeo-rt`'s error macros raise by (`raise_error_id`), skipping the
// registry's by-name probe. Each offset must stay in sync with its row below.
pub const ARGUMENT_ERROR_CLASS: ClassId = exc_id(5);
pub const IO_ERROR_CLASS: ClassId = exc_id(11);
pub const EOF_ERROR_CLASS: ClassId = exc_id(12);
pub const INDEX_ERROR_CLASS: ClassId = exc_id(13);
pub const RANGE_ERROR_CLASS: ClassId = exc_id(18);
pub const FLOAT_DOMAIN_ERROR_CLASS: ClassId = exc_id(19);
pub const REGEXP_ERROR_CLASS: ClassId = exc_id(21);
pub const FIBER_ERROR_CLASS: ClassId = exc_id(25);
pub const THREAD_ERROR_CLASS: ClassId = exc_id(26);
pub const TYPE_ERROR_CLASS: ClassId = exc_id(28);
pub const NO_MEMORY_ERROR_CLASS: ClassId = exc_id(38);

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
const CORE_EXCEPTIONS: [ExceptionClass; 57] = [
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
        name: "TypeError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(29),
        name: "ZeroDivisionError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(30),
        name: "SystemCallError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(31),
        name: "Errno",
        superclass: None,
        is_module: true,
    },
    ExceptionClass {
        id: exc_id(32),
        name: "Math::DomainError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    // `SyntaxError < ScriptError` -- raised by a run-time `eval`
    // when a dynamically-eval'd string fails to parse. Appended AFTER
    // `Math::DomainError` so every pre-existing exception id stays put; like
    // `Math::DomainError` it is registered in the compiler's exception-tail pin
    // rather than in `BUILTIN_EXCEPTIONS_RB` (id-ordering, not a semantic
    // difference).
    ExceptionClass {
        id: exc_id(33),
        name: "SyntaxError",
        superclass: Some(exc_id(1)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(34),
        name: "UncaughtThrowError",
        superclass: Some(exc_id(5)),
        is_module: false,
    },
    // The non-`StandardError` exception tail: a bare `rescue` never catches
    // these (they descend from `Exception` directly), so a program must name
    // them explicitly. `Interrupt < SignalException` mirrors CRuby's SIGINT
    // class. Pinned here (see `analyze::pin_builtin_exceptions_tail`).
    ExceptionClass {
        id: exc_id(35),
        name: "SystemExit",
        superclass: Some(exc_id(0)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(36),
        name: "SignalException",
        superclass: Some(exc_id(0)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(37),
        name: "Interrupt",
        superclass: Some(exc_id(36)),
        is_module: false,
    },
    // The remaining core `Exception`-tree classes CRuby defines (gem- and
    // Ractor-specific ones excluded). `NoMemoryError`/`SecurityError`/
    // `SystemStackError` descend from `Exception` directly (uncaught by a bare
    // `rescue`); the rest refine an existing `StandardError` branch.
    ExceptionClass {
        id: exc_id(38),
        name: "NoMemoryError",
        superclass: Some(exc_id(0)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(39),
        name: "SecurityError",
        superclass: Some(exc_id(0)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(40),
        name: "SystemStackError",
        superclass: Some(exc_id(0)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(41),
        name: "NoMatchingPatternKeyError",
        superclass: Some(exc_id(24)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(42),
        name: "Regexp::TimeoutError",
        superclass: Some(exc_id(21)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(43),
        name: "IO::TimeoutError",
        superclass: Some(exc_id(11)),
        is_module: false,
    },
    // `WeakRef::RefError` -- raised by a `WeakRef` whose referent has been
    // collected. CRuby makes it a plain `StandardError` (exc_id(4)).
    ExceptionClass {
        id: exc_id(44),
        name: "WeakRef::RefError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    // The `Ractor` error tree, raised by the runtime's port model
    // (`zeo-rt::ractor`): closed-port sends/receives, non-creator access,
    // successor violations, and the `Ractor::RemoteError` a `#value`/`#join`
    // relays. `Ractor::ClosedError` descends from `StopIteration` rather than
    // from `Ractor::Error`, which is what lets `Kernel#loop` swallow it.
    ExceptionClass {
        id: exc_id(45),
        name: "Ractor::Error",
        superclass: Some(exc_id(22)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(46),
        name: "Ractor::ClosedError",
        superclass: Some(exc_id(15)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(47),
        name: "Ractor::IsolationError",
        superclass: Some(exc_id(45)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(48),
        name: "Ractor::MovedError",
        superclass: Some(exc_id(45)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(49),
        name: "Ractor::RemoteError",
        superclass: Some(exc_id(45)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(50),
        name: "Ractor::UnsafeError",
        superclass: Some(exc_id(45)),
        is_module: false,
    },
    // The `IO::Buffer` error tree: four states under `RuntimeError`, plus
    // the mask-shape complaint under `ArgumentError` (io_buffer.c's split).
    ExceptionClass {
        id: exc_id(51),
        name: "IO::Buffer::LockedError",
        superclass: Some(exc_id(22)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(52),
        name: "IO::Buffer::AllocationError",
        superclass: Some(exc_id(22)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(53),
        name: "IO::Buffer::AccessError",
        superclass: Some(exc_id(22)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(54),
        name: "IO::Buffer::InvalidatedError",
        superclass: Some(exc_id(22)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(55),
        name: "IO::Buffer::MaskError",
        superclass: Some(exc_id(5)),
        is_module: false,
    },
    // CRuby's internal `fatal`, which is what a detected deadlock raises. The
    // lower-case name is not an accident and not a constant: `fatal` cannot be
    // written in Ruby source at all (a constant must start upper-case, and
    // `const_defined?("fatal")` raises `wrong constant name`), so the class is
    // reachable only as `raise`d and as `e.class`. It descends straight from
    // `Exception`, so `rescue => e` does NOT catch it and `rescue Exception`
    // does -- which is the whole point of the tier.
    ExceptionClass {
        id: exc_id(56),
        name: "fatal",
        superclass: Some(exc_id(0)),
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
pub(crate) const EXCEPTION_INCLUDES: &[(ClassId, &[ClassId])] = &[
    (wait_id(2), &[wait_id(0)]),
    (wait_id(3), &[wait_id(1)]),
    (wait_id(4), &[wait_id(0)]),
    (wait_id(5), &[wait_id(1)]),
];
