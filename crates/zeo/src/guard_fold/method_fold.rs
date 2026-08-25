//! The method-table probes (`respond_to?`, `method_defined?`, named
//! predicates) and the call-shaped guard dispatcher behind them.

use super::pattern::literal_pattern;
use super::version::cmp_fold;
use super::*;
use crate::compiler::ScopeId;
use crate::hir::{Params, Visibility};

/// A literal method-name argument (`:validate_for_resolution` / its string
/// form), the first argument of a `respond_to?`/`method_defined?` probe.
pub(super) fn probe_name(compiler: &Compiler, args: &[ArrayElem]) -> Option<String> {
    let first = match args {
        [ArrayElem::Single(a), ..] => *a,
        _ => return None,
    };
    match &compiler.hir[first] {
        HirNode::SymbolLit(s) => Some(s.clone()),
        HirNode::StringLit(parts) => match parts.as_slice() {
            [StrPart::Lit(s)] => Some(s.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// The class an `X.new` / bare `new` (implicit-self `new` in a class body)
/// receiver is an INSTANCE of, resolved in `cref`. `None` for any other receiver
/// shape (a class object itself, a local, ...), which the caller handles.
fn instance_class(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    receiver: NodeId,
) -> Option<ClassId> {
    match &compiler.hir[receiver] {
        HirNode::New { class_name, .. } => compiler.resolve_class(class_name, cref, box_id),
        // Bare `new` -- `self.new` in a class body, so an instance of the class
        // whose body the guard sits in (the innermost cref entry).
        HirNode::Call {
            receiver: None,
            name,
            args,
            ..
        } if name == "new" && args.is_empty() => cref.last().copied(),
        // A LITERAL is an instance of its own class, and it is how the
        // capability probes are actually written: `''.respond_to?(:bytesize)`,
        // `[].respond_to?(:sum)`. Same fact as `String.new.respond_to?`, spelled
        // the way anyone would spell it.
        HirNode::StringLit(_) => Some(zeo_abi::STRING_CLASS),
        HirNode::SymbolLit(_) => Some(zeo_abi::SYMBOL_CLASS),
        HirNode::IntegerLit(_) => Some(zeo_abi::INTEGER_CLASS),
        HirNode::FloatLit(_) => Some(zeo_abi::FLOAT_CLASS),
        HirNode::ArrayLit(_) => Some(zeo_abi::ARRAY_CLASS),
        HirNode::HashLit(_) => Some(zeo_abi::HASH_CLASS),
        // The three standard streams are IO instances, and highline probes one
        // (`unless STDIN.respond_to? :getbyte`). They are constants rather than
        // classes, so nothing else here would reach them.
        HirNode::ClassRef(n)
            if matches!(n.trim_start_matches("::"), "STDIN" | "STDOUT" | "STDERR") =>
        {
            Some(zeo_abi::IO_CLASS)
        }
        _ => None,
    }
}

/// Whether a BUILTIN class or one of its ancestors declares instance method
/// `name` natively. `builtin_surface` answers per class, without inheritance,
/// so the chain is walked here -- `''.respond_to?(:each_char)` has to see
/// `String`'s own row, `''.respond_to?(:tap)` `Kernel`'s.
///
/// Walks the DECLARED `parent`/`includes` edges rather than `ClassInfo::
/// ancestors`, which `mro::materialize` fills in only after this whole walk has
/// finished -- reading it here would silently see an empty chain and report
/// every inherited method missing.
///
/// `Some(false)` only when EVERY class on the chain has a projected surface: a
/// class not yet migrated to the macro has no list to be absent from, and
/// "missing" would then be a fact about zeo's build rather than about Ruby.
/// When they all do, the absence is real -- `"".respond_to?(:parameterize)` is
/// how test-prof asks whether ActiveSupport has been loaded, and the honest
/// answer is no.
fn builtin_provides_instance_method(
    compiler: &Compiler,
    class: ClassId,
    name: &str,
) -> Option<bool> {
    let mut seen = Vec::new();
    let mut queue = vec![class];
    let mut all_projected = true;
    while let Some(c) = queue.pop() {
        if seen.contains(&c) {
            continue;
        }
        seen.push(c);
        match crate::builtin_surface::surface_for(c) {
            Some(s) if s.instance_methods.contains(&name) => return Some(true),
            Some(_) => {}
            // A BOOTSTRAP class -- the exception hierarchy -- is written in
            // ruby but carries HAND-REGISTERED native rows the projection
            // cannot see (`exception.rs`'s `mark_owned_names`). Neither
            // table holds them, so concluding "not found, fully projected"
            // is a confident false about rows the class really has:
            // `Exception.method_defined?(:detailed_message)` answered false
            // and sent `error_highlight` down its pre-3.2 branch.
            None if compiler.class(c).is_bootstrap => all_projected = false,
            // `Object` and compiled user classes have no projected surface and
            // need none: their methods are COMPILED `def`s, and
            // `method_in_chain` -- which the caller already asked -- is the
            // table that holds them.
            None if c == zeo_abi::OBJECT_CLASS || !compiler.class(c).is_builtin => {}
            None => all_projected = false,
        }
        let info = compiler.class(c);
        queue.extend(info.includes());
        queue.extend(info.parent);
    }
    all_projected.then_some(false)
}

/// Compile-time truth of `recv.respond_to?(:m)`. Answered against the compiled
/// method tables: for an INSTANCE receiver (`X.new`, bare `new`), whether the
/// class has a PUBLIC instance method `m` (respond_to?'s default excludes
/// non-public); for a CLASS receiver, its class methods. A found-public method
/// is `Some(true)`; a definitively-absent instance method is `Some(false)`; a
/// class receiver whose class method is absent stays `None` (a builtin from
/// `Module`/`Object` could still answer it -- don't guess `false`).
fn respond_to_fold(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    receiver: Option<NodeId>,
    args: &[ArrayElem],
) -> Option<bool> {
    let receiver = receiver?;
    let m = probe_name(compiler, args)?;
    if let Some(cls) = instance_class(compiler, cref, box_id, receiver) {
        // Walk-time safe: see `compiled_method_in_chain` -- the materialized
        // table this used to read directly is empty while analyze's own
        // guard folds still run.
        if let Some(sid) = compiled_method_in_chain(compiler, cls, &m) {
            return Some(compiler.scope(sid).visibility == Visibility::Public);
        }
        // NATIVE rows are not in `method_in_chain` -- that table holds
        // compiled Ruby methods. Ask the projected surface for the rest,
        // whether `cls` is itself a builtin or a user class inheriting the
        // method from Object/Kernel.
        return builtin_provides_instance_method(compiler, cls, &m);
    }
    if let Some(cls) = const_receiver_class(compiler, cref, box_id, receiver) {
        // A user-defined class method (compiled `class_methods`) OR a native
        // builtin class method the class declares in its `ruby_class!`/
        // `ruby_module!` (projected into `CLASS_SURFACE` -- e.g.
        // `Process.respond_to?(:_fork)`, which connection_pool's ForkTracker
        // gates on). The projection is why this needs no hardcoded
        // allowlist.
        if compiler.class_method_in_chain(cls, &m).is_some()
            || crate::builtin_surface::provides_class_method(cls, &m)
        {
            return Some(true);
        }
    }
    None
}

/// A compiled instance method visible on `class`'s chain, safe at WALK time.
///
/// `method_in_chain` reads the materialized per-class tables, which
/// `mro::materialize` fills only after the whole statement walk -- so a guard
/// folding DURING the walk (analyze's `static_top_cond`) saw an empty table
/// for a user method registered three statements up, and answered a confident
/// false through `builtin_provides_instance_method`'s all-projected tail. The
/// materialized lookup still goes first: at codegen time it also carries
/// alias/`module_function` copies the own-list walk can't see.
fn compiled_method_in_chain(compiler: &Compiler, class: ClassId, name: &str) -> Option<ScopeId> {
    if let Some((_, sid)) = compiler.method_in_chain(class, name) {
        return Some(sid);
    }
    let mut seen = Vec::new();
    let mut queue = vec![class];
    while let Some(c) = queue.pop() {
        if seen.contains(&c) {
            continue;
        }
        seen.push(c);
        let info = compiler.class(c);
        // A runtime-conditional def is registered but NOT promised -- its
        // presence is often the very question the guard asks (`def added;
        // end unless respond_to? :added`: the def is in `own_methods` by the
        // time its own guard folds). Invisible here; the materialized path
        // above governs at codegen time.
        if let Some(&sid) = info.own_methods.iter().find(|&&s| {
            let sc = compiler.scope(s);
            sc.name == name && !sc.runtime_conditional
        }) {
            return Some(sid);
        }
        queue.extend(info.includes());
        queue.extend(info.parent);
    }
    None
}

/// Compile-time truth of `Recv.method_defined?(:m)` / bare `method_defined?(:m)`
/// (implicit self = the enclosing class): whether the class has a non-private
/// instance method `m` (`method_defined?`'s rule). `None` if the class doesn't
/// resolve.
fn method_defined_fold(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    receiver: Option<NodeId>,
    args: &[ArrayElem],
) -> Option<bool> {
    let m = probe_name(compiler, args)?;
    let cls = match receiver {
        Some(r) => {
            // `X.singleton_class.method_defined?(:m)` asks about X's CLASS
            // methods -- `X.respond_to?(:m)` spelled through the singleton
            // (rbs gates its TypeName parser this way). Presence answers;
            // absence stays undecided, same posture as `respond_to_fold`.
            if let HirNode::Call {
                receiver: Some(inner),
                name,
                args: sc_args,
                block: None,
                ..
            } = &compiler.hir[r]
                && name == "singleton_class"
                && sc_args.is_empty()
                && let Some(cls) = const_receiver_class(compiler, cref, box_id, *inner)
            {
                if compiler.class_method_in_chain(cls, &m).is_some()
                    || crate::builtin_surface::provides_class_method(cls, &m)
                {
                    return Some(true);
                }
                // The singleton also inherits every instance method of
                // `Class` (or `Module` for a module) -- `.method_defined?
                // (:new)` is TRUE for every class, which is exactly the
                // probe rbs writes. Presence answers; absence stays
                // undecided, as above.
                let meta = if compiler.class(cls).is_module {
                    crate::compiler::MODULE_CLASS
                } else {
                    crate::compiler::CLASS_CLASS
                };
                if builtin_provides_instance_method(compiler, meta, &m) == Some(true) {
                    return Some(true);
                }
                return None;
            }
            // Any constant-shaped receiver, not just a bare `ClassRef` --
            // `RBS::TypeName.method_defined?` is a qualified path.
            const_receiver_class(compiler, cref, box_id, r)?
        }
        None => *cref.last()?,
    };
    match compiled_method_in_chain(compiler, cls, &m) {
        Some(sid) => Some(compiler.scope(sid).visibility != Visibility::Private),
        // `method_in_chain` holds COMPILED ruby methods, so native rows are not
        // in it -- neither a builtin's own nor the ones a USER class inherits
        // from Object/Kernel. Without the ancestor walk,
        // `Regexp.method_defined?(:match?)` -- and a user class asked about
        // `:singleton_class` (rspec's 1.8.7 shim guard) -- answered a
        // confident false about methods the class has.
        None => builtin_provides_instance_method(compiler, cls, &m),
    }
}

/// Compile-time truth of a membership test -- the same questions the arms above
/// answer, asked through a list.
///
/// Two receivers answer. A LITERAL array of strings tested against a build-time
/// string is just that comparison (`['opal', 'rubymotion'].include?(RUBY_ENGINE)`,
/// which array.rb gates a whole reopen on). And `X.instance_methods.include?(:m)`
/// is `X.method_defined?(:m)` spelled the long way -- both ask for the public
/// and protected instance methods of `X` and its ancestors.
fn include_fold(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    receiver: NodeId,
    args: &[ArrayElem],
    depth: u32,
) -> Option<bool> {
    let [ArrayElem::Single(wanted)] = args else {
        return None;
    };
    match &compiler.hir[receiver] {
        HirNode::ArrayLit(elems) => {
            let wanted = static_string(compiler, cref, box_id, *wanted, depth)?;
            let mut hit = false;
            for elem in elems {
                // A splat could hold anything, so it takes the whole list with
                // it rather than being skipped.
                let ArrayElem::Single(e) = elem else {
                    return None;
                };
                hit |= static_string(compiler, cref, box_id, *e, depth)? == wanted;
            }
            Some(hit)
        }
        HirNode::Call {
            receiver: Some(cls),
            name,
            args: inner,
            block: None,
            ..
        } if inner.is_empty()
            && matches!(
                name.as_str(),
                "instance_methods" | "public_instance_methods"
            ) =>
        {
            method_defined_fold(compiler, cref, box_id, Some(*cls), args)
        }
        // A build-time STRING receiver: substring containment
        // (`RUBY_PLATFORM.include?('java')`, the JRuby gate spelled without
        // a regexp -- log4r and friends).
        _ => {
            let hay = static_string(compiler, cref, box_id, receiver, depth)?;
            let needle = static_string(compiler, cref, box_id, *wanted, depth)?;
            Some(hay.contains(&needle))
        }
    }
}

/// A call-shaped guard: a version/string comparison, a feature probe
/// (`respond_to?`/`method_defined?`), or `<bool>.freeze` (a no-op on a boolean,
/// which is how `VALIDATES_FOR_RESOLUTION` is written).
pub(super) fn call_fold(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    receiver: Option<NodeId>,
    name: &str,
    args: &[ArrayElem],
    depth: u32,
) -> Option<bool> {
    match name {
        "<" | "<=" | ">" | ">=" | "==" | "!=" => {
            let l = receiver?;
            let [ArrayElem::Single(r)] = args else {
                return None;
            };
            cmp_fold(compiler, cref, box_id, name, l, *r, depth)
        }
        // `Gem.win_platform?` / `Gem.java_platform?` -- platform facts
        // derived from the baked `RUBY_PLATFORM` / seeded `RUBY_ENGINE`,
        // the same way rubygems derives them at runtime (chef and
        // isomorfeus gate whole class trees on these).
        "win_platform?" | "java_platform?" if args.is_empty() => {
            match &compiler.hir[receiver?] {
                HirNode::ClassRef(rn) if rn == "Gem" => {}
                _ => return None,
            }
            if name == "java_platform?" {
                // zeo reports MRI's identity: `RUBY_ENGINE` is "ruby".
                return Some(false);
            }
            win_platform()
        }
        // The ffi gem's own platform facts, same baked source.
        "mac?" | "windows?" | "unix?" | "linux?" | "bsd?" | "solaris?" if args.is_empty() => {
            match &compiler.hir[receiver?] {
                HirNode::ClassRef(rn) if rn.trim_start_matches("::") == "FFI::Platform" => {}
                _ => return None,
            }
            ffi_platform_predicate(name)
        }
        // `unless !defined?(X::VERSION)` -- the pervasive reload guard. Only a
        // condition that folds on its own negates; anything else stays `None`.
        "!" if args.is_empty() => Some(!static_bool(compiler, cref, box_id, receiver?, depth)?),
        "freeze" if args.is_empty() => static_bool(compiler, cref, box_id, receiver?, depth),
        // `if RUBY_PLATFORM =~ /mswin|mingw|windows/` -- the platform gate half
        // the corpus writes, and the reason a windows-only file gets compiled
        // at all. Only a pattern that is literal alternatives folds (see
        // `literal_alternatives`); anything with real regexp syntax in it stays
        // undecided rather than being matched by an approximation.
        "=~" | "match?" => {
            let [ArrayElem::Single(arg)] = args else {
                return None;
            };
            let recv = receiver?;
            // Either side may hold the pattern: `RUBY_PLATFORM =~ /x/` and
            // `/x/ =~ RUBY_PLATFORM` are the same question.
            let (subject, pattern) = match literal_pattern(compiler, *arg) {
                Some(p) => (recv, p),
                None => (*arg, literal_pattern(compiler, recv)?),
            };
            // `=~` WRITES `$~`, and folding it away loses that. A build gate
            // asks about a fact (`RUBY_PLATFORM`), so the trade is worth it
            // there and the emitter re-runs the match for its effect. A
            // STRING LITERAL subject is not a gate -- it is ordinary code,
            // where `if "ab" =~ /a/` must leave `$~` set.
            if name == "=~" && matches!(compiler.hir[subject], HirNode::StringLit(_)) {
                return None;
            }
            let subject = static_string(compiler, cref, box_id, subject, depth)?;
            pattern.matches(&subject)
        }
        // `if RUBY_VERSION.start_with?('1.9')` -- the same build-time question
        // the comparison operators above answer, asked by prefix. Ruby takes any
        // number of candidates and is true if ANY matches; a Regexp candidate
        // (also legal) reduces to no string, so the whole probe stays `None`.
        "start_with?" | "end_with?" => {
            let s = static_string(compiler, cref, box_id, receiver?, depth)?;
            let mut hit = false;
            for arg in args {
                let ArrayElem::Single(a) = arg else {
                    return None;
                };
                let candidate = static_string(compiler, cref, box_id, *a, depth)?;
                hit |= if name == "start_with?" {
                    s.starts_with(&candidate)
                } else {
                    s.ends_with(&candidate)
                };
            }
            Some(hit)
        }
        // `if RUBY_PLATFORM['linux']` -- `String#[]` hands back the match or
        // nil, so AS A CONDITION it is a containment test. Only reached when
        // both sides reduce to strings, so an Array or Hash `[]` never lands
        // here.
        "[]" => {
            let [ArrayElem::Single(a)] = args else {
                return None;
            };
            let s = static_string(compiler, cref, box_id, receiver?, depth)?;
            let part = static_string(compiler, cref, box_id, *a, depth)?;
            Some(s.contains(&part))
        }
        "include?" => include_fold(compiler, cref, box_id, receiver?, args, depth),
        "respond_to?" => respond_to_fold(compiler, cref, box_id, receiver, args),
        "const_defined?" => const_defined_fold(compiler, cref, box_id, receiver, args),
        "method_defined?" | "public_method_defined?" => {
            method_defined_fold(compiler, cref, box_id, receiver, args)
        }
        _ => predicate_fold(compiler, cref, box_id, receiver, name, args, depth),
    }
}

/// The class or module a CONSTANT-PATH receiver names: a bare `Process`
/// (`ClassRef`) or a top-anchored `::Process`, which reads as
/// `QualifiedConstRead("Object", "Process")` because a top-level constant lives
/// on `Object`. Both resolve at the root scope; everything else resolves at the
/// guard's own cref. `None` for any receiver that isn't a constant.
fn const_receiver_class(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    receiver: NodeId,
) -> Option<ClassId> {
    match &compiler.hir[receiver] {
        HirNode::ClassRef(name) => match name.strip_prefix("::") {
            Some(rooted) => compiler.resolve_class(rooted, &[], box_id),
            None => compiler.resolve_class(name, cref, box_id),
        },
        HirNode::QualifiedConstRead(scope, name) if scope == "Object" => {
            compiler.resolve_class(name, &[], box_id)
        }
        HirNode::QualifiedConstRead(scope, name) => {
            compiler.resolve_class(&format!("{scope}::{name}"), cref, box_id)
        }
        _ => None,
    }
}

/// Whether a method takes nothing at all, so a bare `Recv.name` runs its body
/// with no argument to have changed the answer.
fn takes_no_arguments(params: &Params) -> bool {
    params.required.is_empty()
        && params.destructures.is_empty()
        && params.optional.is_empty()
        && params.rest.is_none()
        && params.post.is_empty()
        && params.keywords.is_empty()
        && params.keyword_rest.is_none()
        && params.block.is_none()
}

/// Every definition of `name` that a `Recv.name` on this module could reach and
/// that this file can read: the module's own class methods (`def self.name`),
/// its own instance methods, and those of the modules it extends.
///
/// Instance methods belong here because `extend self` and `module_function` are
/// the usual way a module makes its predicates callable on itself, and both
/// stay runtime calls in zeo -- the method lands in `own_methods` and nothing
/// static ever moves it. Rather than model the singleton ancestry to work out
/// which definition wins, [`predicate_fold`] reads them all and insists they
/// agree, which answers the question without needing to know.
fn predicate_definitions(compiler: &Compiler, owner: ClassId, name: &str) -> Vec<ScopeId> {
    let info = compiler.class(owner);
    let mut out: Vec<ScopeId> = Vec::new();
    let tables = info
        .own_class_methods
        .iter()
        .chain(&info.own_methods)
        .chain(
            info.extends
                .iter()
                .filter(|m| **m != owner)
                .flat_map(|m| &compiler.class(*m).own_methods),
        );
    for &sid in tables {
        if compiler.scope(sid).name == name && !out.contains(&sid) {
            out.push(sid);
        }
    }
    out
}

/// Whether `owner` is the only class in the program that defines `name`.
///
/// A receiverless call dispatches on the live `self`, which may be an instance
/// of a subclass or an includer rather than of `owner` itself. Reading only
/// `owner`'s definition would then answer for the wrong body. With no other
/// definition of the name anywhere, there is no other body to reach.
///
/// Deliberately not gated on [`Compiler::may_be_patched_at_runtime`]: that
/// answers a different question (might this name be REWRITTEN at run time),
/// which no predicate fold models -- one computed `define_method` anywhere sets
/// it program-wide, and every real gem has one. The one runtime-installed shape
/// that WOULD change the answer here is a conditional `def`, and
/// [`predicate_fold`] declines on that directly.
fn defines_name_alone(compiler: &Compiler, owner: ClassId, name: &str) -> bool {
    compiler.classes.iter().enumerate().all(|(i, info)| {
        i == owner.0 as usize
            || !info
                .own_methods
                .iter()
                .chain(&info.own_class_methods)
                .any(|&sid| compiler.scope(sid).name == name)
    })
}

/// Compile-time value of a zero-argument predicate on a module -- the shape a
/// compat gate takes once a gem gives its build-time question a name:
/// `if Sass::Util.rbx?`, `if Lutaml::Model::RuntimeCompatibility.opal?`. What
/// those predicates test is what the folders above already decide
/// (`RUBY_ENGINE == "rbx"`); naming it is the only thing that hid it.
///
/// EVERY definition the module carries has to fold, and to the SAME value --
/// see [`predicate_definitions`] for why reading all of them is what makes it
/// safe not to know which one a call reaches.
///
/// Only the VALUE folds. The method is still compiled and still callable, so
/// what a folded guard drops is one call whose whole effect was to compute a
/// constant -- including, for the memoized spelling, the caching of it (see
/// [`method_value_bool`]).
fn predicate_fold(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    receiver: Option<NodeId>,
    name: &str,
    args: &[ArrayElem],
    depth: u32,
) -> Option<bool> {
    if !args.is_empty() {
        return None;
    }
    let owner = match receiver {
        Some(r) => const_receiver_class(compiler, cref, box_id, r)?,
        // No receiver: `self`, which inside a method body is an instance of the
        // enclosing class -- or of something below it that may have overridden
        // the name. `defines_name_alone` is what rules that out, and sass needs
        // it: `ruby1_8?` opens with a bare `ironruby?`.
        None => {
            let owner = *cref.last()?;
            if !defines_name_alone(compiler, owner, name) {
                return None;
            }
            owner
        }
    };
    let definitions = predicate_definitions(compiler, owner, name);
    if definitions.is_empty() {
        return None;
    }
    let mut answer: Option<bool> = None;
    for sid in definitions {
        let scope = compiler.scope(sid);
        // A `def` under a guard zeo could not decide is registered but not
        // promised (`analyze::register_conditional_defs`), so its body is not
        // the answer -- whether it is installed at all is the same undecided
        // question that put it there.
        if !takes_no_arguments(&scope.params) || scope.runtime_conditional {
            return None;
        }
        // Folded where the method was WRITTEN: a bare constant in its body
        // resolves in its own lexical scope, not at the guard's.
        let home = compiler.cref_of(Some(scope.defining_class));
        let value = method_value_bool(compiler, &home, box_id, &scope.body, depth)?;
        if answer.is_some_and(|a| a != value) {
            return None;
        }
        answer = Some(value);
    }
    answer
}

/// The value a zero-argument method body produces on EVERY call, when that is
/// one of the decidable forms.
///
/// Either the body is a single expression that folds, or it is the memoized
/// spelling gems write a build-time predicate in:
///
/// ```ruby
/// def rbx?
///   return @rbx if defined?(@rbx)
///   @rbx = RUBY_ENGINE == "rbx"
/// end
/// ```
///
/// A memo over a constant expression answers that constant on every call, first
/// or later, so the guard reads the same either way -- and the cached ivar is
/// unobservable outside the memo that wrote it, which is why a guard that folds
/// may skip writing it.
fn method_value_bool(
    compiler: &Compiler,
    cref: &[ClassId],
    box_id: u32,
    body: &[NodeId],
    depth: u32,
) -> Option<bool> {
    let (&last, leading) = body.split_last()?;
    let memo = match leading {
        [] => None,
        [guard] => Some(memo_guard_ivar(compiler, *guard)?),
        _ => return None,
    };
    let value = match (&compiler.hir[last], memo) {
        // An assignment's value is the value assigned. Allowed only for the
        // ivar the memo guard read -- that pairing is what makes the write the
        // cache rather than an effect.
        (HirNode::IvarWrite(n, v), Some(memo)) if n == memo => *v,
        (_, None) => last,
        _ => return None,
    };
    static_bool(compiler, cref, box_id, value, depth).or_else(|| literal_truth(compiler, value))
}

/// The ivar a `return @x if defined?(@x)` memo guard reads, or `None` for any
/// other statement.
fn memo_guard_ivar(compiler: &Compiler, stmt: NodeId) -> Option<&str> {
    let HirNode::If {
        cond,
        then_body,
        else_body,
    } = &compiler.hir[stmt]
    else {
        return None;
    };
    if !else_body.is_empty() {
        return None;
    }
    let HirNode::Defined(probe) = &compiler.hir[*cond] else {
        return None;
    };
    let HirNode::IvarRead(probed) = &compiler.hir[*probe] else {
        return None;
    };
    let [returned] = then_body.as_slice() else {
        return None;
    };
    let HirNode::Return(Some(value)) = &compiler.hir[*returned] else {
        return None;
    };
    let HirNode::IvarRead(read) = &compiler.hir[*value] else {
        return None;
    };
    (read == probed).then_some(probed.as_str())
}
