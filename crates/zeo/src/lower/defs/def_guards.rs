//! Load-time static guards for definition lowering: the platform/engine
//! conditions a def-site `if` can decide at compile time.

use super::*;

/// A class/module-body `if`/`unless` guard zeo can decide at COMPILE time --
/// [`static_bool`]'s literals, plus the two probes that gate definitions all
/// over the gem graph: `defined?(C)` for a constant the runtime always provides,
/// and `RUBY_VERSION <cmp> "x"` against the one version zeo targets.
///
/// Deciding it matters more than it saves: a `def` in each branch of a guard
/// that stays dynamic leaves TWO definitions of one name in the body, and the
/// later one simply wins -- so `if RUBY_VERSION >= "3.4."` picked the pre-3.4
/// method. This is the same three-valued evaluation
/// [`eval_static_class_self_guard`] does for a `class << self` body, over prism
/// nodes rather than HIR because the class-body path folds before lowering.
pub(crate) fn static_guard(node: &Node<'_>) -> Option<bool> {
    if let Some(b) = static_bool(node) {
        return Some(b);
    }
    if let Some(paren) = node.as_parentheses_node() {
        let stmts = paren.body()?.as_statements_node()?;
        let body: Vec<_> = stmts.body().iter().collect();
        if let [only] = body.as_slice() {
            return static_guard(only);
        }
    }
    if let Some(defined) = node.as_defined_node() {
        let name = defined.value().as_constant_read_node()?.name();
        let name = String::from_utf8_lossy(name.as_slice());
        // Only the runtime-provided constants decide here; any OTHER name is
        // UNDECIDABLE at lowering, not false -- the program may well define
        // it, and `analyze`'s `splice_decidable_ifs` folds the surviving
        // `if` with the whole-program view. Answering false here dropped
        // live branches (`if defined?(SomeDep)` with SomeDep loaded).
        return ALWAYS_DEFINED_CONSTS
            .contains(&name.as_ref())
            .then_some(true);
    }
    // `a && b` / `a || b`: three-valued short-circuit. One decided side can
    // decide the whole guard even when the other stays unknown -- `x && false`
    // is falsy for EITHER x (it returns x when x is falsy, false otherwise),
    // and `x || true` truthy the same way.
    if let Some(and) = node.as_and_node() {
        return match (static_guard(&and.left()), static_guard(&and.right())) {
            (Some(false), _) => Some(false),
            (Some(true), r) => r,
            (None, Some(false)) => Some(false),
            (None, _) => None,
        };
    }
    if let Some(or) = node.as_or_node() {
        return match (static_guard(&or.left()), static_guard(&or.right())) {
            (Some(true), _) => Some(true),
            (Some(false), r) => r,
            (None, Some(true)) => Some(true),
            (None, _) => None,
        };
    }
    let call = node.as_call_node()?;
    let op = String::from_utf8_lossy(call.name().as_slice()).into_owned();
    let args: Vec<Node<'_>> = call
        .arguments()
        .map(|a| a.arguments().iter().collect())
        .unwrap_or_default();
    if op == "!" && args.is_empty() {
        return Some(!static_guard(&call.receiver()?)?);
    }
    // `Gem.win_platform?` / `Gem.java_platform?` -- the platform facts gems
    // gate whole FFI declaration blocks on, answered from the same baked
    // values `guard_fold` uses so lower and analyze always pick one branch.
    if matches!(op.as_str(), "win_platform?" | "java_platform?") && args.is_empty() {
        let recv = call.receiver()?;
        if String::from_utf8_lossy(recv.as_constant_read_node()?.name().as_slice()) != "Gem" {
            return None;
        }
        if op == "java_platform?" {
            return Some(false); // zeo reports MRI's identity
        }
        return crate::guard_fold::win_platform();
    }
    // `FFI::Platform.mac?` and its siblings -- the ffi gem's platform facts
    // (smartcard's `Word` typedef picks its width this way), answered from
    // the same baked values so lower and analyze pick one branch.
    if matches!(
        op.as_str(),
        "mac?" | "windows?" | "unix?" | "linux?" | "bsd?" | "solaris?"
    ) && args.is_empty()
    {
        let recv = call.receiver()?;
        let path = crate::lower::ffi::const_path_string(&recv)?;
        if path.trim_start_matches("::") != "FFI::Platform" {
            return None;
        }
        return crate::guard_fold::ffi_platform_predicate(&op);
    }
    match op.as_str() {
        // Both comparison families reduce their operands the same way ruby
        // would dispatch them: `String#<=>` is bytewise (the RUBY_VERSION /
        // RUBY_PLATFORM gates), `Integer#<=>` numeric (`FFI::Platform::
        // ADDRESS_SIZE == 64`, which gates vips' `:GType` typedef).
        ">=" | ">" | "<" | "<=" | "==" | "!=" => {
            let recv = call.receiver()?;
            let [rhs] = args.as_slice() else {
                return None;
            };
            use std::cmp::Ordering::{Equal, Greater, Less};
            let ord = if let (Some(l), Some(r)) = (guard_string(&recv), guard_string(rhs)) {
                l.as_bytes().cmp(r.as_bytes())
            } else if let (Some(l), Some(r)) = (guard_integer(&recv), guard_integer(rhs)) {
                l.cmp(&r)
            } else {
                return None;
            };
            Some(match op.as_str() {
                ">=" => ord != Less,
                ">" => ord == Greater,
                "<" => ord == Less,
                "<=" => ord != Greater,
                "==" => ord == Equal,
                _ => ord != Equal,
            })
        }
        // `RUBY_PLATFORM =~ /mswin|mingw/` -- the guard half the windows-only
        // FFI files sit under. `LiteralPattern` is guard_fold's own parser
        // (only exact character tests fold), so the two stages agree; either
        // side may hold the pattern.
        "=~" | "!~" | "match?" | "match" => {
            let [arg] = args.as_slice() else {
                return None;
            };
            let recv = call.receiver()?;
            let (subject, pattern) = match guard_pattern(arg) {
                Some(p) => (guard_string(&recv)?, p),
                None => (guard_string(arg)?, guard_pattern(&recv)?),
            };
            let hit = pattern.matches(&subject)?;
            Some(if op == "!~" { !hit } else { hit })
        }
        // `RUBY_PLATFORM.include?('mswin')` and the prefix/suffix spellings of
        // the same question. Ruby's `start_with?`/`end_with?` take any number
        // of candidates and answer true if ANY matches.
        "include?" | "start_with?" | "end_with?" => {
            let s = guard_string(&call.receiver()?)?;
            if args.is_empty() {
                return None;
            }
            let mut hit = false;
            for arg in &args {
                let candidate = guard_string(arg)?;
                hit |= match op.as_str() {
                    "include?" => s.contains(&candidate),
                    "start_with?" => s.starts_with(&candidate),
                    _ => s.ends_with(&candidate),
                };
            }
            Some(hit)
        }
        _ => None,
    }
}

/// The branch a class-body `case` selects when both its subject and EVERY
/// `when` condition up to the match are compile-time facts -- the case-shaped
/// spelling of [`static_guard`]'s question. `Some(None)` is a decided case
/// with nothing to run (no `when` matched and no `else`); `None` is a case
/// that has to stay.
///
/// Every condition before the winning one has to be decidable too: an
/// undecidable earlier `when` might have matched first, so the branch after it
/// is not the answer.
pub(super) fn static_case_branch<'a>(
    case_node: &ruby_prism::CaseNode<'a>,
) -> Option<Option<Node<'a>>> {
    let subject = case_node.predicate()?;
    for clause in case_node.conditions().iter() {
        let when = clause.as_when_node()?;
        for cond in when.conditions().iter() {
            if case_condition_matches(&cond, &subject)? {
                return Some(when.statements().map(|s| s.as_node()));
            }
        }
    }
    Some(case_node.else_clause().map(|e| e.as_node()))
}

/// `cond === subject` for the literal forms a platform `case` is written with:
/// a regexp, a string, an integer. `None` for anything else -- the case stays.
fn case_condition_matches(cond: &Node<'_>, subject: &Node<'_>) -> Option<bool> {
    if let Some(pattern) = guard_pattern(cond) {
        return pattern.matches(&guard_string(subject)?);
    }
    if let Some(want) = guard_string(cond) {
        return Some(guard_string(subject)? == want);
    }
    Some(guard_integer(cond)? == guard_integer(subject)?)
}

/// Reduce a prism node to a compile-time STRING for [`static_guard`]: a plain
/// string literal, or a constant ruby seeds into every program
/// (`RUBY_VERSION`, `RUBY_PLATFORM`, ...) -- `guard_fold::seeded_string_const`,
/// so the lower-stage fold and the analyze-stage fold read the same values.
fn guard_string(node: &Node<'_>) -> Option<String> {
    if let Some(s) = node.as_string_node() {
        return String::from_utf8(s.unescaped().to_vec()).ok();
    }
    // `FFI::Platform::ARCH == 'x86_64'` -- the ffi gem's own strings, baked
    // from the same platform the seeded constants come from.
    if let Some(path) = crate::lower::ffi::const_path_string(node)
        && let Some(leaf) = path
            .trim_start_matches("::")
            .strip_prefix("FFI::Platform::")
    {
        return crate::guard_fold::ffi_platform_string(leaf);
    }
    if let Some(key) = rbconfig_key(node) {
        return crate::guard_fold::rbconfig_string(&key).map(str::to_string);
    }
    let name = String::from_utf8_lossy(node.as_constant_read_node()?.name().as_slice());
    crate::guard_fold::seeded_string_const(&name)
}

/// The literal key of a `RbConfig::CONFIG['host_os']` read, or `None` for any
/// other node. Both `[]` and `fetch` spell it.
pub(crate) fn rbconfig_key(node: &Node<'_>) -> Option<String> {
    let call = node.as_call_node()?;
    if !matches!(call.name().as_slice(), b"[]" | b"fetch") {
        return None;
    }
    let path = crate::lower::ffi::const_path_string(&call.receiver()?)?;
    if path.trim_start_matches("::") != "RbConfig::CONFIG" {
        return None;
    }
    let mut args = call.arguments()?.arguments().iter();
    let (arg, None) = (args.next()?, args.next()) else {
        return None;
    };
    String::from_utf8(arg.as_string_node()?.unescaped().to_vec()).ok()
}

/// Reduce a prism node to a compile-time INTEGER for [`static_guard`]: an
/// integer literal, or the `FFI::Platform` size constants the `ffi` gem's
/// platform-gated `typedef`s test (LP64 on every target zeo builds for).
fn guard_integer(node: &Node<'_>) -> Option<i64> {
    if let Some(int) = node.as_integer_node() {
        let value = int.value();
        let (negative, digits) = value.to_u32_digits();
        return crate::lower::literals::assemble_i64(negative, digits);
    }
    let leaf = constant_path_name(node)
        .ok()?
        .trim_start_matches("::")
        .strip_prefix("FFI::Platform::")?
        .to_string();
    crate::guard_fold::ffi_platform_integer(&leaf)
}

/// A regexp LITERAL parsed into `guard_fold`'s exact-fold pattern; `None` for
/// an interpolated pattern or one carrying real regexp syntax.
fn guard_pattern(node: &Node<'_>) -> Option<crate::guard_fold::LiteralPattern> {
    let re = node.as_regular_expression_node()?;
    let src = String::from_utf8(re.unescaped().to_vec()).ok()?;
    crate::guard_fold::LiteralPattern::parse(
        &src,
        &crate::hir::RegexpFlags {
            ignore_case: re.is_ignore_case(),
            extended: re.is_extended(),
            multiline: re.is_multi_line(),
            // Ignored by `parse`; `Source` is the flagless spelling.
            encoding: zeo_abi::RegexpEncoding::Source,
        },
    )
}

/// Three-valued static evaluation of a `class << self` conditional-def guard,
/// enough for the platform/version probes that gate class-method definitions in
/// the stdlib/gem graph: literal `true`/`false`/`nil`, `defined?(C)` for a
/// runtime-provided constant, and `RUBY_VERSION <cmp> "x"` (String#<=>
/// lexicographic, matching how Ruby compares these version strings). `None` when
/// the guard depends on runtime state -- the caller then keeps the runtime `if`.
pub(super) fn eval_static_class_self_guard(hir: &Hir, cond: NodeId) -> Option<bool> {
    match &hir[cond] {
        HirNode::BoolLit(b) => Some(*b),
        HirNode::NilLit => Some(false),
        HirNode::Defined(inner) => match &hir[*inner] {
            // Same three-valued honesty as `static_guard`: only a
            // runtime-provided constant decides; an unknown name keeps the
            // runtime `if` rather than dropping a live branch.
            HirNode::ClassRef(name) => ALWAYS_DEFINED_CONSTS
                .contains(&name.as_str())
                .then_some(true),
            _ => None,
        },
        HirNode::Call {
            receiver: Some(recv),
            name,
            args,
            ..
        } => {
            let HirNode::ClassRef(cname) = &hir[*recv] else {
                return None;
            };
            if cname != "RUBY_VERSION" {
                return None;
            }
            let [ArrayElem::Single(arg)] = args.as_slice() else {
                return None;
            };
            let HirNode::StringLit(parts) = &hir[*arg] else {
                return None;
            };
            let [StrPart::Lit(rhs)] = parts.as_slice() else {
                return None;
            };
            let ord = TARGET_RUBY_VERSION.cmp(rhs.as_str());
            use std::cmp::Ordering::{Equal, Greater, Less};
            Some(match name.as_str() {
                ">=" => ord != Less,
                ">" => ord == Greater,
                "<" => ord == Less,
                "<=" => ord != Greater,
                "==" => ord == Equal,
                "!=" => ord != Equal,
                _ => return None,
            })
        }
        _ => None,
    }
}
