//! Build-time static-guard evaluation for the loader: decides platform/engine
//! guards (`RUBY_PLATFORM =~ /mswin|mingw/`, `Gem.win_platform?`,
//! `RUBY_ENGINE == 'jruby'`, ...) while FILES ARE STILL BEING LOADED, before
//! any `Compiler` exists -- so it reads only constants the build bakes in.
//! Deliberately much narrower than [`crate::guard_fold`], which runs later
//! with the whole program in hand.

/// A build-time-decidable platform guard's answer, or `None` for every other
/// condition. Deliberately much narrower than [`crate::guard_fold`]: this runs
/// while FILES ARE STILL BEING LOADED, before any `Compiler` exists, so it can
/// only read constants the build bakes in.
///
/// The one shape it needs is the one gems actually write:
/// `RUBY_PLATFORM =~ /mswin|mingw|windows/`, and its `match?` and `==`
/// spellings. That decides whether a windows-only file is part of this program
/// -- mixlib-shellout requires one from inside `class ShellOut`, and its
/// `:dword` FFI types exist nowhere else.
fn baked_constant(node: &ruby_prism::Node<'_>) -> Option<&'static str> {
    let name = node.as_constant_read_node()?;
    match name.name().as_slice() {
        b"RUBY_PLATFORM" => Some(env!("ZEO_RUBY_PLATFORM")),
        // Mirrors `zeo_rt::bootstrap`'s `ENGINE = "ruby"`: zeo targets CRuby
        // semantics, so an engine-gated require is statically decidable.
        b"RUBY_ENGINE" => Some("ruby"),
        b"RUBY_VERSION" | b"RUBY_ENGINE_VERSION" => Some(zeo_abi::RUBY_VERSION),
        _ => None,
    }
}

/// A build-time-known STRING subject: a baked constant, or a
/// `RbConfig::CONFIG['host_os']`-style read of an entry this build bakes --
/// the older spelling of the same platform question, answered from the very
/// value the generated program's `RbConfig::CONFIG` will hold.
pub(super) fn baked_subject(node: &ruby_prism::Node<'_>) -> Option<&'static str> {
    if let Some(s) = baked_constant(node) {
        return Some(s);
    }
    let key = crate::lower::defs::rbconfig_key(node)?;
    crate::guard_fold::rbconfig_string(&key)
}

/// Whether this build's platform is a windows one -- the answer every
/// spelling of the oldest platform test resolves to.
pub(super) fn build_is_windows() -> bool {
    let plat = env!("ZEO_RUBY_PLATFORM");
    ["mswin", "mingw", "windows"]
        .iter()
        .any(|w| plat.contains(w))
}

/// `Some(<baked constant> matches <pattern>)` for `RUBY_PLATFORM =~ /mswin|
/// mingw|windows/` and its `match?` spelling, either operand order.
///
/// Only a pattern of `|`-separated LITERAL text folds, so the match is a
/// substring test that cannot disagree with a regexp engine -- the same rule,
/// and the same reason, as `guard_fold::literal_alternatives`.
fn platform_match(node: &ruby_prism::Node<'_>) -> Option<bool> {
    let call = node.as_call_node()?;
    if !matches!(call.name().as_slice(), b"=~" | b"match?") {
        return None;
    }
    let recv = call.receiver()?;
    let mut args = call.arguments()?.arguments().iter();
    let (arg, None) = (args.next()?, args.next()) else {
        return None;
    };
    let (subject, pattern) = match baked_subject(&recv) {
        Some(s) => (s, arg),
        None => (baked_subject(&arg)?, recv),
    };
    literal_alt_match(subject, &pattern)
}

/// `Some(subject matches re)` for a regexp of `|`-separated LITERAL text --
/// a substring test that cannot disagree with a regexp engine. Anything with
/// syntax or flags is `None`.
fn literal_alt_match(subject: &str, pattern: &ruby_prism::Node<'_>) -> Option<bool> {
    let re = pattern.as_regular_expression_node()?;
    if re.is_ignore_case() || re.is_extended() || re.is_multi_line() {
        return None;
    }
    let src = String::from_utf8_lossy(re.unescaped()).into_owned();
    const SYNTAX: &[char] = &[
        '\\', '^', '$', '.', '[', ']', '(', ')', '*', '+', '?', '{', '}',
    ];
    if src.is_empty() || src.contains(SYNTAX) {
        return None;
    }
    let alts: Vec<&str> = src.split('|').collect();
    if alts.iter().any(|a| a.is_empty()) {
        return None;
    }
    Some(alts.iter().any(|a| subject.contains(a)))
}

pub(super) fn literal_when_match(subject: &str, cond: &ruby_prism::Node<'_>) -> Option<bool> {
    if let Some(s) = cond.as_string_node() {
        return Some(s.unescaped() == subject.as_bytes());
    }
    literal_alt_match(subject, cond)
}

/// Three-valued evaluation of a `require`-guard expression. `Some(true)`/
/// `Some(false)` when a platform guard is statically decidable; `None` when it
/// depends on runtime state (the branch stays live and its requires splice as
/// before). Only the idioms that gate platform-specific requires in the stdlib/
/// gem graph are modeled -- literal `true`/`false`/`nil`, `RUBY_ENGINE == "..."`
/// (and `!=`), `!x`, and `&&`/`||` over those. Everything else is `None`, so
/// this only ever PRUNES a provably-dead branch; it never drops a live require.
pub(super) fn eval_static_guard(node: &ruby_prism::Node<'_>) -> Option<bool> {
    if node.as_true_node().is_some() {
        return Some(true);
    }
    if node.as_false_node().is_some() || node.as_nil_node().is_some() {
        return Some(false);
    }
    if let Some(paren) = node.as_parentheses_node() {
        let stmts = paren.body()?.as_statements_node()?;
        let body: Vec<_> = stmts.body().iter().collect();
        return match body.as_slice() {
            [only] => eval_static_guard(only),
            _ => None,
        };
    }
    if let Some(and) = node.as_and_node() {
        return match (
            eval_static_guard(&and.left()),
            eval_static_guard(&and.right()),
        ) {
            (Some(false), _) | (_, Some(false)) => Some(false),
            (Some(true), Some(true)) => Some(true),
            _ => None,
        };
    }
    if let Some(or) = node.as_or_node() {
        return match (
            eval_static_guard(&or.left()),
            eval_static_guard(&or.right()),
        ) {
            (Some(true), _) | (_, Some(true)) => Some(true),
            (Some(false), Some(false)) => Some(false),
            _ => None,
        };
    }
    if let Some(b) = platform_match(node) {
        return Some(b);
    }
    // `defined?(RUBY_ENGINE)` -- the interpreter always defines it, so the
    // question is only ever "am I old enough to have it", answered at build
    // time. binding_of_caller opens on `defined?(RUBY_ENGINE) && RUBY_ENGINE ==
    // "ruby"`, and without this the `and` stayed unfolded and every `elsif`
    // branch loaded -- including the JRuby one, whose `class
    // org::jruby::runtime::ThreadContext` no CRuby build could ever compile.
    if let Some(d) = node.as_defined_node()
        && baked_constant(&d.value()).is_some()
    {
        return Some(true);
    }
    // `if File::ALT_SEPARATOR` -- the oldest "am I on windows" test there is,
    // and how sys-filesystem picks its half. The constant is `"\\"` on windows
    // and `nil` everywhere else, so its TRUTH is a property of the build.
    if let Some(path) = node.as_constant_path_node()
        && path
            .name()
            .is_some_and(|n| n.as_slice() == b"ALT_SEPARATOR")
        && path
            .parent()
            .and_then(|p| {
                p.as_constant_read_node()
                    .map(|c| c.name().as_slice() == b"File")
            })
            .unwrap_or(false)
    {
        return Some(build_is_windows());
    }
    // `defined?(JRUBY_VERSION)` and its family: a version constant only
    // ANOTHER interpreter defines. zeo targets CRuby, where the answer is a
    // build-time fact -- no gem defines a foreign engine's version constant
    // on CRuby, and thread_safe/concurrent-ruby gate whole require graphs on
    // exactly this test.
    if let Some(d) = node.as_defined_node()
        && let Some(read) = d.value().as_constant_read_node()
        && matches!(
            read.name().as_slice(),
            b"JRUBY_VERSION" | b"RUBINIUS_VERSION" | b"MACRUBY_VERSION" | b"MRUBY_VERSION"
        )
    {
        return Some(false);
    }
    if let Some(call) = node.as_call_node() {
        let name = call.name().as_slice();
        if name == b"!" && call.arguments().is_none() {
            return call
                .receiver()
                .and_then(|r| eval_static_guard(&r))
                .map(|b| !b);
        }
        if matches!(name, b"==" | b"!=")
            && let (Some(recv), Some(args)) = (call.receiver(), call.arguments())
        {
            let arg_list: Vec<_> = args.arguments().iter().collect();
            if let [only] = arg_list.as_slice() {
                let eq = baked_string_eq(&recv, only).or_else(|| baked_string_eq(only, &recv));
                if let Some(eq) = eq {
                    return Some(if name == b"==" { eq } else { !eq });
                }
            }
        }
        if let Some(b) = platform_predicate(&call) {
            return Some(b);
        }
    }
    None
}

/// `Some(<baked constant> == lit)` when `a` reads a build-baked constant and
/// `b` is a string literal; `None` otherwise. Order-sensitive -- the caller
/// tries both operand orders so `RUBY_ENGINE == 'x'` and `'x' == RUBY_ENGINE`
/// both resolve. Baking the SUBJECT rather than one constant name is what
/// decides `RUBY_PLATFORM == "java"` too, JRuby's other spelling.
fn baked_string_eq(a: &ruby_prism::Node<'_>, b: &ruby_prism::Node<'_>) -> Option<bool> {
    let subject = baked_subject(a)?;
    Some(b.as_string_node()?.unescaped() == subject.as_bytes())
}

/// The zero-argument platform predicates whose answer is a property of the
/// build: `Gem.win_platform?`, and the `FFI::Platform.windows?/mac?/unix?`
/// family zeo's own ffi gem provides.
fn platform_predicate(call: &ruby_prism::CallNode<'_>) -> Option<bool> {
    if call.arguments().is_some() || call.block().is_some() {
        return None;
    }
    let recv = call.receiver()?;
    let name = call.name().as_slice();
    if let Some(c) = recv.as_constant_read_node()
        && c.name().as_slice() == b"Gem"
        && name == b"win_platform?"
    {
        return Some(build_is_windows());
    }
    let path = recv.as_constant_path_node()?;
    if path.name()?.as_slice() != b"Platform"
        || !path
            .parent()
            .and_then(|p| {
                p.as_constant_read_node()
                    .map(|c| c.name().as_slice() == b"FFI")
            })
            .unwrap_or(false)
    {
        return None;
    }
    match name {
        b"windows?" => Some(build_is_windows()),
        b"mac?" => Some(env!("ZEO_RUBY_PLATFORM").contains("darwin")),
        b"unix?" => Some(!build_is_windows()),
        _ => None,
    }
}
