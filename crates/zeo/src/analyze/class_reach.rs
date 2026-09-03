//! Which always-on builtin classes a program can reach.
//!
//! An always-on builtin's method table is tens of kilobytes, and the
//! smallest program there is used to carry every one of them. A `puts 1`
//! that ships `Ractor`, `Marshal`, `TracePoint` and `Pathname` pays for
//! classes nothing in it can name. `cargo xtask size` prices them;
//! `docs/explanation/binary-size.md` carries the numbers.
//!
//! Narrowing is safe to be WRONG in one direction only. A class kept for
//! nothing costs bytes; a class dropped that the program does reach aborts at
//! the first dispatch, naming itself (`builtins::registered_table`). So every
//! rule here over-approximates on purpose, and the reachability question is
//! asked as "is there any channel at all", never "does this call happen".
//!
//! Three channels reach a class:
//!
//! 1. **The program names it.** A constant read, a superclass, an `include`.
//! 2. **A value arrives as one.** `1.to_s` needs `String` with `String`
//!    written nowhere, so [`SEED`] is unconditional; and a builtin row can
//!    RETURN an instance of a class nothing named -- `caller_locations` hands
//!    back `Thread::Backtrace::Location` -- which is what [`METHOD_SEEDS`]
//!    covers, keyed by the method name rather than by any type.
//! 3. **Reflection hands out every class there is.** `Marshal.load` rebuilds
//!    an arbitrary object graph; `ObjectSpace.each_object` enumerates the
//!    heap; `RubyVM::AbstractSyntaxTree.parse` answers with a literal of
//!    whatever class the TEXT names. Those are hatches, not edges: the answer
//!    becomes "everything".
//!
//! Require-GATED builtins are not narrowed here at all. Their feature gate
//! already answers the same question more precisely, and a gated extension
//! reaches its own nested classes through Rust the compiler cannot see.

use crate::compiler::{ClassId, Compiler, FSet};
use crate::hir::{ArrayElem, Hir, HirNode};
use crate::types::TyKind;

/// The classes a value can arrive as with the program naming nothing.
///
/// Every literal has one, every operator answers one, and `Object`'s own
/// chain is under all of them. `Enumerator` is here because any `each`
/// without a block returns one, and the whole `Exception` hierarchy is
/// unconditional because `raise` reaches it from anywhere.
const SEED: &[ClassId] = &[
    zeo_abi::OBJECT_CLASS,
    zeo_abi::BASIC_OBJECT_CLASS,
    zeo_abi::KERNEL_CLASS,
    zeo_abi::MODULE_CLASS,
    zeo_abi::CLASS_CLASS,
    zeo_abi::COMPARABLE_CLASS,
    zeo_abi::ENUMERABLE_CLASS,
    zeo_abi::ENUMERATOR_CLASS,
    // The enumerator family is built by ruby, not by the program: `chunk_while`
    // makes a Generator, `each_entry` a Yielder, an endless source a Producer.
    // None of them is ever named.
    zeo_abi::ENUMERATOR_GENERATOR_CLASS,
    zeo_abi::YIELDER_CLASS,
    zeo_abi::ENUMERATOR_PRODUCER_CLASS,
    zeo_abi::ENUMERATOR_PRODUCT_CLASS,
    // `e1 + e2` chains and `(1..10) % 3` steps. Both are OPERATORS on classes
    // that are themselves unconditional, so no call-name rule separates them
    // from arithmetic.
    zeo_abi::ENUMERATOR_CHAIN_CLASS,
    zeo_abi::ENUMERATOR_ARITHMETIC_SEQUENCE_CLASS,
    zeo_abi::NUMERIC_CLASS,
    zeo_abi::INTEGER_CLASS,
    zeo_abi::FLOAT_CLASS,
    zeo_abi::STRING_CLASS,
    zeo_abi::SYMBOL_CLASS,
    zeo_abi::ARRAY_CLASS,
    zeo_abi::HASH_CLASS,
    zeo_abi::RANGE_CLASS,
    zeo_abi::NIL_CLASS,
    zeo_abi::TRUE_CLASS,
    zeo_abi::FALSE_CLASS,
    zeo_abi::PROC_CLASS,
    zeo_abi::IO_CLASS,
    zeo_abi::BINDING_CLASS,
    zeo_abi::BACKTRACE_CLASS,
    zeo_abi::BACKTRACE_LOCATION_CLASS,
    zeo_abi::WARNING_MODULE,
];

/// Method names that hand back an instance of a class the program never
/// names -- the return-type channel, which no syntactic scan of constants
/// can see.
///
/// Keyed by NAME with no receiver test on purpose. A user class with its own
/// `stat` keeps `File::Stat` for nothing, which costs bytes; a receiver test
/// that guessed wrong would abort the program.
const METHOD_SEEDS: &[(&str, &[ClassId])] = &[
    // `gets`/`readlines` with no receiver read ARGF, and `$<` IS ARGF.
    ("gets", &[zeo_abi::ARGF_CLASS, zeo_abi::FILE_CLASS]),
    ("readline", &[zeo_abi::ARGF_CLASS]),
    ("readlines", &[zeo_abi::ARGF_CLASS, zeo_abi::FILE_CLASS]),
    // `Kernel#open` answers a File.
    (
        "open",
        &[zeo_abi::FILE_CLASS, zeo_abi::FILE_CONSTANTS_MODULE],
    ),
    ("stat", &[zeo_abi::FILE_STAT_CLASS]),
    // `Kernel#test(?e, path)` asks the same surface `FileTest` does.
    (
        "test",
        &[
            zeo_abi::FILE_CLASS,
            zeo_abi::FILE_TEST_MODULE,
            zeo_abi::FILE_STAT_CLASS,
        ],
    ),
    ("lstat", &[zeo_abi::FILE_STAT_CLASS]),
    // The frame surfaces.
    ("caller", &[zeo_abi::BACKTRACE_CLASS]),
    (
        "caller_locations",
        &[zeo_abi::BACKTRACE_LOCATION_CLASS, zeo_abi::BACKTRACE_CLASS],
    ),
    (
        "backtrace_locations",
        &[zeo_abi::BACKTRACE_LOCATION_CLASS, zeo_abi::BACKTRACE_CLASS],
    ),
    ("binding", &[zeo_abi::BINDING_CLASS]),
    ("set_trace_func", &[zeo_abi::TRACEPOINT_CLASS]),
    // A negative base to a fractional power is a Complex, and `quo` and the
    // `to_r` family are Rational. Neither ever names its class.
    ("**", &[zeo_abi::COMPLEX_CLASS, zeo_abi::RATIONAL_CLASS]),
    ("quo", &[zeo_abi::RATIONAL_CLASS]),
    ("to_r", &[zeo_abi::RATIONAL_CLASS]),
    ("rationalize", &[zeo_abi::RATIONAL_CLASS]),
    ("numerator", &[zeo_abi::RATIONAL_CLASS]),
    ("denominator", &[zeo_abi::RATIONAL_CLASS]),
    ("to_c", &[zeo_abi::COMPLEX_CLASS]),
    ("Complex", &[zeo_abi::COMPLEX_CLASS]),
    ("Rational", &[zeo_abi::RATIONAL_CLASS]),
    ("polar", &[zeo_abi::COMPLEX_CLASS]),
    ("rectangular", &[zeo_abi::COMPLEX_CLASS]),
    // Encoding arrives from any string that reports or changes its own.
    ("encoding", &[zeo_abi::ENCODING_CLASS]),
    ("force_encoding", &[zeo_abi::ENCODING_CLASS]),
    ("set_encoding", &[zeo_abi::ENCODING_CLASS]),
    ("external_encoding", &[zeo_abi::ENCODING_CLASS]),
    ("internal_encoding", &[zeo_abi::ENCODING_CLASS]),
    ("encode", &[zeo_abi::ENCODING_CLASS]),
    ("unicode_normalize", &[zeo_abi::ENCODING_CLASS]),
    // The enumerator family. `Enumerator` itself is seeded; these are the
    // nested classes an instance of the family arrives as.
    ("lazy", &[zeo_abi::LAZY_CLASS]),
    ("chain", &[zeo_abi::ENUMERATOR_CHAIN_CLASS]),
    ("step", &[zeo_abi::ENUMERATOR_ARITHMETIC_SEQUENCE_CLASS]),
    ("next", &[zeo_abi::FIBER_CLASS]),
    ("peek", &[zeo_abi::FIBER_CLASS]),
    // `method(:x)` and `instance_method(:x)`.
    ("method", &[zeo_abi::METHOD_CLASS]),
    ("public_method", &[zeo_abi::METHOD_CLASS]),
    ("singleton_method", &[zeo_abi::METHOD_CLASS]),
    ("bind", &[zeo_abi::METHOD_CLASS]),
    ("bind_call", &[zeo_abi::METHOD_CLASS]),
    ("instance_method", &[zeo_abi::UNBOUND_METHOD_CLASS]),
    ("public_instance_method", &[zeo_abi::UNBOUND_METHOD_CLASS]),
    ("unbind", &[zeo_abi::UNBOUND_METHOD_CLASS]),
    ("to_proc", &[zeo_abi::PROC_CLASS]),
    // Randomness: `rand`/`sample`/`shuffle` all draw on the default Random.
    (
        "rand",
        &[
            zeo_abi::RANDOM_CLASS,
            zeo_abi::RANDOM_BASE_CLASS,
            zeo_abi::RANDOM_FORMATTER_MODULE,
        ],
    ),
    (
        "srand",
        &[zeo_abi::RANDOM_CLASS, zeo_abi::RANDOM_BASE_CLASS],
    ),
    (
        "sample",
        &[zeo_abi::RANDOM_CLASS, zeo_abi::RANDOM_BASE_CLASS],
    ),
    (
        "shuffle",
        &[zeo_abi::RANDOM_CLASS, zeo_abi::RANDOM_BASE_CLASS],
    ),
    // A child process reports through `$?`, a Process::Status.
    //
    // `Process` itself rides along because the Kernel spelling of each of
    // these FORWARDS to it: `Kernel#exec` is a send to `Process.exec`, so a
    // program that writes `exec` and never writes `Process` still reaches
    // Process's table -- and dropping it aborted the program rather than
    // raising the `Errno::ENOENT` it was rescuing.
    ("system", &[zeo_abi::PROCESS_CLASS, zeo_abi::PROCESS_STATUS_CLASS]),
    ("spawn", &[zeo_abi::PROCESS_CLASS, zeo_abi::PROCESS_STATUS_CLASS]),
    ("`", &[zeo_abi::PROCESS_CLASS, zeo_abi::PROCESS_STATUS_CLASS]),
    ("exec", &[zeo_abi::PROCESS_CLASS, zeo_abi::PROCESS_STATUS_CLASS]),
    ("fork", &[zeo_abi::PROCESS_CLASS, zeo_abi::PROCESS_STATUS_CLASS]),
    ("waitpid", &[zeo_abi::PROCESS_CLASS, zeo_abi::PROCESS_STATUS_CLASS]),
    ("trap", &[zeo_abi::SIGNAL_MODULE]),
    // `Process.detach` answers a Process::Waiter, which IS a Thread.
    (
        "detach",
        &[zeo_abi::PROCESS_WAITER_CLASS, zeo_abi::THREAD_CLASS],
    ),
    (
        "wait",
        &[zeo_abi::PROCESS_WAITER_CLASS, zeo_abi::THREAD_CLASS],
    ),
    // A file timestamp is a Time, and a Date converts to one.
    ("mtime", &[zeo_abi::TIME_CLASS]),
    ("atime", &[zeo_abi::TIME_CLASS]),
    ("ctime", &[zeo_abi::TIME_CLASS]),
    ("birthtime", &[zeo_abi::TIME_CLASS]),
    ("to_time", &[zeo_abi::TIME_CLASS]),
    ("utime", &[zeo_abi::TIME_CLASS]),
    // `__dir__` answers through the same File surface `File.dirname` does.
    ("__dir__", &[zeo_abi::FILE_CLASS]),
    ("to_set", &[zeo_abi::SET_CLASS]),
    ("warn", &[zeo_abi::WARNING_MODULE]),
    ("refine", &[zeo_abi::REFINEMENT_CLASS]),
    ("using", &[zeo_abi::REFINEMENT_CLASS]),
    ("console_mode", &[zeo_abi::CONSOLE_MODE_CLASS]),
    ("raw", &[zeo_abi::CONSOLE_MODE_CLASS]),
    ("raw!", &[zeo_abi::CONSOLE_MODE_CLASS]),
    ("cooked", &[zeo_abi::CONSOLE_MODE_CLASS]),
    ("Pathname", &[zeo_abi::PATHNAME_CLASS]),
];

/// Every method name that reads or writes a match: each leaves `$~` set, so
/// the program holds a `MatchData` with `Regexp` written nowhere.
const MATCH_METHODS: &[&str] = &[
    "=~",
    "!~",
    "match",
    "match?",
    "scan",
    "sub",
    "sub!",
    "gsub",
    "gsub!",
    "split",
    "partition",
    "rpartition",
    "start_with?",
    "end_with?",
    "index",
    "rindex",
    "slice",
    "slice!",
    "grep",
    "grep_v",
    "last_match",
];

/// Classes that arrive TOGETHER: reaching the left reaches the right, through
/// a row that names neither.
///
/// `Process.times` answers a `Process::Tms` and `$?` a `Process::Status`;
/// `Rational#i` answers a `Complex` and `Complex#to_r` a `Rational`. Each
/// pair is a closure edge, not a seed -- it fires only when its left side is
/// already reachable.
const PAIRED: &[(ClassId, &[ClassId])] = &[
    (
        zeo_abi::PROCESS_CLASS,
        &[
            zeo_abi::PROCESS_TMS_CLASS,
            zeo_abi::PROCESS_STATUS_CLASS,
            zeo_abi::PROCESS_WAITER_CLASS,
        ],
    ),
    (zeo_abi::RATIONAL_CLASS, &[zeo_abi::COMPLEX_CLASS]),
    (zeo_abi::COMPLEX_CLASS, &[zeo_abi::RATIONAL_CLASS]),
    (zeo_abi::REGEXP_CLASS, &[zeo_abi::MATCH_DATA_CLASS]),
    (zeo_abi::MATCH_DATA_CLASS, &[zeo_abi::REGEXP_CLASS]),
    (zeo_abi::THREAD_CLASS, &[zeo_abi::MUTEX_CLASS]),
];

/// Globals that ARE an instance of a class the program names nowhere.
const GLOBAL_SEEDS: &[(&str, &[ClassId])] = &[
    ("$~", &[zeo_abi::MATCH_DATA_CLASS, zeo_abi::REGEXP_CLASS]),
    ("$&", &[zeo_abi::MATCH_DATA_CLASS, zeo_abi::REGEXP_CLASS]),
    ("$`", &[zeo_abi::MATCH_DATA_CLASS, zeo_abi::REGEXP_CLASS]),
    ("$'", &[zeo_abi::MATCH_DATA_CLASS, zeo_abi::REGEXP_CLASS]),
    ("$<", &[zeo_abi::ARGF_CLASS]),
    ("$?", &[zeo_abi::PROCESS_STATUS_CLASS]),
    ("$stdin", &[zeo_abi::IO_CLASS]),
    ("$stdout", &[zeo_abi::IO_CLASS]),
    ("$stderr", &[zeo_abi::IO_CLASS]),
];

/// An always-on class a REQUIRE-GATED extension hands back, which the program
/// itself may never name.
///
/// The loop over `BUILTINS` below seeds the gated class itself; this is the
/// other direction -- `PTY.spawn` answers two `File`s and `PTY.check` a
/// `Process::Status`, so a program that requires `pty` and writes neither name
/// still dispatches through both. Dropping one of those tables is not a wrong
/// answer, it is an abort at the first method call.
const FEATURE_SEEDS: &[(&str, &[ClassId])] = &[(
    "pty",
    &[
        zeo_abi::FILE_CLASS,
        zeo_abi::FILE_CONSTANTS_MODULE,
        zeo_abi::IO_CLASS,
        zeo_abi::PROCESS_STATUS_CLASS,
        zeo_abi::THREAD_CLASS,
    ],
)];

/// The narrowed set, or `None` for "every class is reachable".
///
/// `None` is the answer whenever a run-time compile or a reflection hatch
/// makes the question unanswerable, and it is what a caller that asks before
/// `analyze` runs also sees.
pub(crate) fn resolve(compiler: &Compiler) -> Option<FSet<ClassId>> {
    if compiler.runtime_eval
        || compiler.hir.mentions_rubyvm_parser()
        || hands_out_every_class(&compiler.hir)
    {
        return None;
    }
    let mut set: FSet<ClassId> = SEED.iter().copied().collect();
    for exc in zeo_abi::EXCEPTION_CLASSES {
        set.insert(exc.id);
    }
    mentioned_classes(&compiler.hir, &mut set);
    method_seeded_classes(&compiler.hir, &mut set);
    inferred_classes(compiler, &mut set);
    // A gated builtin is not narrowed here, but the always-on classes BEHIND
    // it are: `Etc::Passwd` is a real `Struct` subclass, and a program that
    // requires `etc` and names `Struct` nowhere still dispatches through it.
    for b in zeo_abi::BUILTINS {
        if b.feature.is_some() && compiler.feature_active(ClassId(b.id.0)) {
            set.insert(b.id);
        }
    }
    for (feature, ids) in FEATURE_SEEDS {
        if compiler.hir.activated_features.contains(*feature) {
            set.extend(ids.iter().copied());
        }
    }
    // A user class's resolved chain names every builtin it inherits or mixes
    // in, whatever spelling reached it.
    for class in &compiler.classes {
        if class.is_builtin || class.is_bootstrap {
            continue;
        }
        set.extend(class.ancestors.iter().copied());
    }
    close_over_ancestry(&mut set);
    Some(set)
}

/// Whether the program holds a surface that can produce an object of ANY
/// class without naming it -- reflection wide enough that no edge analysis
/// applies.
///
/// Each of these is a whole channel rather than a call: `Marshal.load`
/// rebuilds an arbitrary graph, `ObjectSpace.each_object` walks the heap,
/// `Module#constants` enumerates every name there is, and a `const_get` or a
/// `send` whose argument is COMPUTED names something no scan can read.
fn hands_out_every_class(hir: &Hir) -> bool {
    hir.nodes().iter().any(|node| match node {
        HirNode::ClassRef(n) | HirNode::New { class_name: n, .. } => {
            n == "Marshal" || n == "ObjectSpace"
        }
        HirNode::QualifiedConstRead(scope, n) | HirNode::ConstReadOrNil(Some(scope), n) => {
            matches!(scope.as_str(), "Marshal" | "ObjectSpace")
                || matches!(n.as_str(), "Marshal" | "ObjectSpace")
        }
        HirNode::Call { name, args, .. } => match name.as_str() {
            // Every constant there is, by name.
            "constants"
            | "const_get"
            | "const_source_location"
            | "const_defined?"
            | "remove_const" => !matches!(
                args.first().map(ArrayElem::node_id).map(|a| &hir[a]),
                Some(HirNode::SymbolLit(_) | HirNode::StringLit(_))
            ),
            // A COMPUTED send names a method no scan can read, and the row it
            // lands on can answer with anything. A literal one is ordinary.
            "send" | "__send__" | "public_send" => args
                .first()
                .map(ArrayElem::node_id)
                .is_none_or(|a| hir.sent_name(a).is_none()),
            _ => false,
        },
        _ => false,
    })
}

/// Every builtin whose NAME the program writes, in any constant position.
fn mentioned_classes(hir: &Hir, set: &mut FSet<ClassId>) {
    // Every segmentation of a qualified path. `Enumerator::Lazy` reaches the
    // nested class AND the namespace it hangs off; a root-qualified
    // `::Math::PI` is written with a leading `::` that no builtin name has,
    // and reading it as one name lost `Math` -- and `Math::PI` with it,
    // silently, because a dropped table takes its CONSTANTS too.
    fn note<'a>(n: &'a str, names: &mut FSet<&'a str>) {
        let n = n.strip_prefix("::").unwrap_or(n);
        names.insert(n);
        for (i, _) in n.match_indices("::") {
            names.insert(&n[..i]);
            names.insert(&n[i + 2..]);
        }
    }
    let mut names: FSet<&str> = FSet::default();
    for node in hir.nodes() {
        match node {
            HirNode::ClassRef(n)
            | HirNode::New { class_name: n, .. }
            | HirNode::Include(n)
            | HirNode::Extend(n)
            | HirNode::Prepend(n)
            | HirNode::ClassMethodPrepend(n)
            | HirNode::Using(n) => note(n, &mut names),
            HirNode::QualifiedConstRead(scope, n) => {
                note(scope, &mut names);
                note(n, &mut names);
            }
            HirNode::DynConstRead { name, .. }
            | HirNode::DynConstWrite { name, .. }
            | HirNode::ConstWrite {
                scope: None, name, ..
            } => note(name, &mut names),
            HirNode::ConstWrite {
                scope: Some(scope),
                name,
                ..
            } => {
                note(scope, &mut names);
                note(name, &mut names);
            }
            HirNode::ConstReadOrNil(scope, n) => {
                if let Some(scope) = scope {
                    note(scope, &mut names);
                }
                note(n, &mut names);
            }
            _ => {}
        }
    }
    // The set holds every path a mention could have spelled, so a builtin is
    // in when its own name is written, when a nested name under it is, or
    // when its LAST segment alone is (a reopen from inside the namespace
    // writes the short form). All three over-approximate on purpose.
    for b in zeo_abi::BUILTINS {
        // `ARGF.class` is a real builtin NAME (CRuby reports it that way), and
        // no `::` split reaches it -- the program writes `ARGF`.
        let short = b
            .name
            .strip_suffix(".class")
            .unwrap_or(b.name)
            .rsplit("::")
            .next()
            .unwrap_or(b.name);
        if names.contains(b.name) || names.contains(short) {
            set.insert(b.id);
        }
    }
    for (alias, id) in zeo_abi::TOP_LEVEL_ALIASES {
        if names.contains(*alias) {
            set.insert(*id);
        }
    }
}

/// Every class a called method can hand back without the program naming it.
fn method_seeded_classes(hir: &Hir, set: &mut FSet<ClassId>) {
    let mut called: FSet<&str> = FSet::default();
    for node in hir.nodes() {
        match node {
            HirNode::Call { name, args, .. } => {
                called.insert(name.as_str());
                // A LITERAL `send(:Pathname, ...)` is that call written the
                // other way, and the row it lands on returns the same class.
                if matches!(name.as_str(), "send" | "__send__" | "public_send")
                    && let Some(sent) = args.first().and_then(|a| hir.sent_name(a.node_id()))
                {
                    called.insert(sent);
                }
            }
            HirNode::DefMethod { name, .. } => {
                called.insert(name.as_str());
            }
            // A regexp literal is the one shape that reaches `Regexp` with no
            // call and no constant at all.
            // `refine`/`using` are their own nodes, so no call name carries
            // them.
            HirNode::Refine { .. } | HirNode::Using(_) => {
                set.insert(zeo_abi::REFINEMENT_CLASS);
            }
            HirNode::RegexpLit(..) => {
                set.insert(zeo_abi::REGEXP_CLASS);
                set.insert(zeo_abi::MATCH_DATA_CLASS);
            }
            HirNode::RationalLit { .. } => {
                set.insert(zeo_abi::RATIONAL_CLASS);
            }
            HirNode::ImaginaryLit(_) => {
                set.insert(zeo_abi::COMPLEX_CLASS);
            }
            // `$~`/`$&`/`` $` ``/`$'`/`$1`.. lower to their OWN node, not to a
            // global read -- they name the last-match slot.
            HirNode::LastMatchRef(_) => {
                set.insert(zeo_abi::MATCH_DATA_CLASS);
                set.insert(zeo_abi::REGEXP_CLASS);
            }
            HirNode::GlobalRead(g) | HirNode::GlobalWrite(g, _) => {
                let numbered = g.len() == 2 && g.as_bytes()[1].is_ascii_digit();
                if numbered {
                    set.insert(zeo_abi::MATCH_DATA_CLASS);
                    set.insert(zeo_abi::REGEXP_CLASS);
                }
                for (name, ids) in GLOBAL_SEEDS {
                    if g.as_str() == *name {
                        set.extend(ids.iter().copied());
                    }
                }
            }
            _ => {}
        }
    }
    // `__END__` gives the program a `DATA` constant, which IS an open File.
    if hir.data_section.is_some() {
        set.insert(zeo_abi::FILE_CLASS);
        set.insert(zeo_abi::FILE_CONSTANTS_MODULE);
    }
    for (name, ids) in METHOD_SEEDS {
        if called.contains(name) {
            set.extend(ids.iter().copied());
        }
    }
    if MATCH_METHODS.iter().any(|m| called.contains(m)) {
        set.insert(zeo_abi::REGEXP_CLASS);
        set.insert(zeo_abi::MATCH_DATA_CLASS);
    }
}

/// Every class a static type names -- the answer analyze already computed.
fn inferred_classes(compiler: &Compiler, set: &mut FSet<ClassId>) {
    let mut note = |ty: &TyKind| match ty {
        TyKind::Object(cid) | TyKind::ClassObj(cid) => {
            set.insert(ClassId(cid.0));
        }
        TyKind::Regexp => {
            set.insert(zeo_abi::REGEXP_CLASS);
        }
        TyKind::MatchData => {
            set.insert(zeo_abi::MATCH_DATA_CLASS);
        }
        TyKind::Fiber => {
            set.insert(zeo_abi::FIBER_CLASS);
        }
        TyKind::Thread => {
            set.insert(zeo_abi::THREAD_CLASS);
        }
        TyKind::Mutex => {
            set.insert(zeo_abi::MUTEX_CLASS);
        }
        TyKind::Queue => {
            set.insert(zeo_abi::QUEUE_CLASS);
        }
        TyKind::Ractor => {
            set.insert(zeo_abi::RACTOR_CLASS);
        }
        _ => {}
    };
    for scope in &compiler.scopes {
        for ty in scope.local_types.values() {
            note(ty);
        }
    }
}

/// Add every class an already-reachable one drags in: its whole declared
/// ancestry, its mixins, and the modules it extends or prepends.
///
/// A class cannot answer a call without the chain behind it, so this closure
/// is not an over-approximation -- it is the rest of the same fact.
fn close_over_ancestry(set: &mut FSet<ClassId>) {
    let mut queue: Vec<ClassId> = set.iter().copied().collect();
    while let Some(id) = queue.pop() {
        let mut add = |c: ClassId, queue: &mut Vec<ClassId>| {
            if set.insert(c) {
                queue.push(c);
            }
        };
        for a in zeo_abi::declared_ancestors(id) {
            add(a, &mut queue);
        }
        for (owner, mods) in zeo_abi::BUILTIN_EXTENDS
            .iter()
            .chain(zeo_abi::BUILTIN_PREPENDS)
        {
            if *owner == id {
                for m in *mods {
                    add(*m, &mut queue);
                }
            }
        }
        for (owner, together) in PAIRED {
            if ClassId(owner.0) == id {
                for c in *together {
                    add(ClassId(c.0), &mut queue);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The narrowed set for a program, or `None` for "everything".
    fn reach(src: &str) -> Option<FSet<ClassId>> {
        crate::analyze_program(src, &Default::default())
            .expect("compiles")
            .compiler
            .reachable_builtins
    }

    fn by_name(name: &str) -> ClassId {
        if name == "Object" {
            return ClassId(zeo_abi::OBJECT_CLASS.0);
        }
        let id = zeo_abi::BUILTINS
            .iter()
            .find(|b| b.name == name)
            .unwrap_or_else(|| panic!("no builtin named `{name}`"))
            .id;
        ClassId(id.0)
    }

    fn holds(src: &str, name: &str) -> bool {
        reach(src).is_none_or(|set| set.contains(&by_name(name)))
    }

    /// The whole point: the smallest program there is reaches almost nothing,
    /// and every class it cannot reach is a method table it need not carry.
    #[test]
    fn the_smallest_program_reaches_almost_nothing() {
        let set = reach("puts 1").expect("no hatch, so a narrowed answer");
        for gone in [
            "Marshal",
            "Ractor",
            "Pathname",
            "TracePoint",
            "Time",
            "Random",
            "Regexp",
            "Thread",
            "Set",
            "Math",
            "Dir",
            "Struct",
            "GC",
        ] {
            assert!(
                !set.contains(&by_name(gone)),
                "`puts 1` cannot reach {gone}, so carrying its table is dead weight"
            );
        }
        // And what it CAN reach stays: a value arrives as one of these with
        // the program naming none of them.
        for kept in [
            "Integer",
            "String",
            "Array",
            "Hash",
            "Symbol",
            "NilClass",
            "Float",
            "Kernel",
            "Comparable",
            "Enumerable",
            "Enumerator",
        ] {
            assert!(
                set.contains(&by_name(kept)),
                "a value can arrive as {kept} without being named -- dropping \
                 its table aborts the program"
            );
        }
    }

    /// Naming a class is the plainest channel there is, in every constant
    /// spelling.
    #[test]
    fn naming_a_class_reaches_it() {
        assert!(holds("puts Time.now.year", "Time"));
        assert!(holds("puts Math::PI", "Math"));
        assert!(holds("Thread.new { 1 }.join", "Thread"));
        assert!(holds("p Enumerator::Lazy", "Enumerator::Lazy"));
        // A qualified read reaches the NAMESPACE too.
        assert!(holds("p Process::Status", "Process"));
        // And the top-level alias for a nested class.
        assert!(holds("q = Queue.new\nq << 1\n", "Thread::Queue"));
    }

    /// A class a called row RETURNS, with the program naming it nowhere. No
    /// scan of constants can see any of these.
    #[test]
    fn a_returned_class_is_reached_without_being_named() {
        assert!(holds("p caller_locations", "Thread::Backtrace::Location"));
        assert!(holds("p binding.local_variables", "Binding"));
        assert!(holds("p method(:puts)", "Method"));
        assert!(holds("p 1.quo(2)", "Rational"));
        assert!(holds("p \"x\".encoding", "Encoding"));
        assert!(holds("p [1, 2].lazy.first", "Enumerator::Lazy"));
        assert!(holds("p rand(2)", "Random"));
        assert!(holds("p File.mtime(__FILE__)", "Time"));
        // A LITERAL `send` is the same call written the other way.
        assert!(holds("p send(:binding).class", "Binding"));
    }

    /// A literal reaches its own class with no call and no constant.
    #[test]
    fn a_literal_reaches_its_own_class() {
        assert!(holds("p(/x/.source)", "Regexp"));
        assert!(holds("p(/x/.source)", "MatchData"));
        assert!(holds("p 1r", "Rational"));
        assert!(holds("p 1i", "Complex"));
        // A match leaves `$~` set, so the program holds a MatchData whether
        // or not it names one.
        assert!(holds("p \"ab\".sub(\"a\", \"c\")", "MatchData"));
        assert!(holds("p $~", "MatchData"));
    }

    /// Reflection wide enough to produce ANY class is a hatch, not an edge:
    /// the answer becomes "everything", because nothing syntactic bounds it.
    #[test]
    fn a_give_everything_hatch_answers_everything() {
        for hatch in [
            "p Marshal.load(Marshal.dump([1]))",
            "ObjectSpace.each_object(Class) { |c| p c }",
            "p Object.constants.size",
            "name = ARGV[0]\np Object.const_get(name)\n",
            "name = ARGV[0]\np Object.const_source_location(name)\n",
            "m = ARGV[0]\np 1.send(m)\n",
            "p RubyVM::AbstractSyntaxTree.parse(\"1r\")",
            "p eval(\"1 + 1\")",
        ] {
            assert!(
                reach(hatch).is_none(),
                "`{hatch}` can name a class no scan can see, so narrowing it \
                 would abort the program at the first dispatch"
            );
        }
    }

    /// The same surfaces with a LITERAL argument are ordinary calls, and must
    /// NOT give up the whole set -- that is what makes the hatch worth having.
    #[test]
    fn a_literal_argument_is_not_a_hatch() {
        for ordinary in [
            "p Object.const_get(:Integer)",
            "p 1.send(:to_s)",
            "p 1.public_send(\"succ\")",
        ] {
            assert!(
                reach(ordinary).is_some(),
                "`{ordinary}` names what it reaches, so it keeps the narrowed set"
            );
        }
    }

    /// A reachable class drags its whole chain in -- it cannot answer a call
    /// without it.
    #[test]
    fn the_set_closes_over_the_ancestry() {
        let set = reach("p Enumerator::Lazy").expect("narrowed");
        for behind in [
            "Enumerator",
            "Enumerable",
            "Object",
            "Kernel",
            "BasicObject",
        ] {
            assert!(
                set.contains(&by_name(behind)),
                "Enumerator::Lazy cannot answer a call without {behind} behind it"
            );
        }
    }

    /// A user class's own chain names every builtin it inherits or mixes in,
    /// whatever spelling reached it.
    #[test]
    fn a_user_classs_chain_is_reached() {
        let set = reach("class Point < Struct.new(:x)\n  include Comparable\nend\np Point\n")
            .expect("narrowed");
        assert!(set.contains(&by_name("Struct")));
        assert!(set.contains(&by_name("Comparable")));
    }

    /// `refine`/`using` are their own HIR nodes, so no call name carries
    /// them -- the one shape a call-name table cannot see.
    #[test]
    fn a_refinement_is_reached_through_its_own_node() {
        let src = "module M\n  refine String do\n    def shout = upcase\n  end\nend\n\
                   using M\nputs \"hi\".shout\n";
        assert!(holds(src, "Refinement"));
    }

    /// A require-GATED class is not narrowed here at all -- but the always-on
    /// classes BEHIND it are, and `Etc::Passwd` is a real `Struct` subclass.
    #[test]
    fn a_gated_class_drags_its_always_on_ancestry_in() {
        let set = reach("require \"etc\"\np Etc::Passwd.members\n").expect("narrowed");
        assert!(
            set.contains(&by_name("Struct")),
            "Etc::Passwd dispatches through Struct with `Struct` written nowhere"
        );
    }
}
