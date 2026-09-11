//! Runtime-class machinery: expressibility tests, synthesized
//! Struct/module classes, and the lowering of `Class.new`-minted classes,
//! reopens, and their bodies.

use super::*;

/// Whether the program ASSIGNS this constant a value anywhere already lowered
/// (`B = Box.new`, `Foo = Class.new`) -- which makes it a value-holding
/// constant rather than the name of a compile-time class. "Already lowered" is
/// the point: the assignment is lowered before any later statement that reads
/// it, which is the same "defined earlier in the file" rule
/// `Compiler::resolve_class` applies, and `Hir::const_write_values_in_scope`
/// is maintained as writes are pushed so it keeps that property.
///
/// Asked FROM THE CREF BEING LOWERED, so a same-named constant in an unrelated
/// namespace does not answer -- see `Hir::const_writes`.
pub(crate) fn const_is_assigned(hir: &Hir, name: &str) -> bool {
    !hir.const_write_values_in_scope(name).is_empty()
}

/// Whether a bare `class Name ... end` (no superclass clause) REOPENS a
/// constant holding a class minted at run time (`Name = Data.define(:x)`,
/// `Struct.new`, `Class.new`) -- the shapes that lower to `Name.class_eval {
/// body }` rather than to a fresh static class. A value constant (`C = 7`)
/// does not qualify, and a `class`-defined `Name` in the same scope keeps the
/// static path.
///
/// A bare name asks the CURRENT cref only. That is ruby's `class` statement:
/// it looks the name up in the cref's own constant table and mints there on a
/// miss, so a same-named class or Data constant in an ENCLOSING scope is
/// invisible to it. Asking the lexical chain instead sent `class Program`
/// inside `Sow::GL::Op` -- where `Program = Data.define(:vertex)` sits -- to
/// the static path because an unrelated `Sow::Program` was `class`-defined
/// two scopes out. A qualified `class A::B` keeps the lexical walk: its tables
/// key by spelling, and what `A` names is not this statement's question.
pub(crate) fn reopens_a_runtime_class(hir: &Hir, name: &str) -> bool {
    if !crate::constpath::ConstPath::parse(name).is_bare() {
        return const_holds_runtime_class(hir, name) && !const_is_class_def(hir, name);
    }
    !hir.class_defined_here(name)
        && hir
            .const_write_values_here(name)
            .iter()
            .any(|&v| value_mints_runtime_class(hir, v))
}

/// A direct local-variable write in a class body -- the one statement the
/// runtime forms cannot express at all, rather than merely having to re-spell.
/// A class body opens its OWN scope, while the block the runtime form becomes
/// closes over the enclosing one, so `y = 1` here would assign the caller's
/// `y`. Everything else (`include`, a visibility directive, `alias`, a nested
/// class) has a runtime spelling -- see `transform_runtime_class_body`.
fn writes_a_local(stmt: &Node<'_>) -> bool {
    stmt.as_local_variable_write_node().is_some()
        || stmt.as_local_variable_operator_write_node().is_some()
        || stmt.as_local_variable_and_write_node().is_some()
        || stmt.as_local_variable_or_write_node().is_some()
        || stmt.as_multi_write_node().is_some()
}

/// [`writes_a_local`] over a whole class body.
pub(crate) fn runtime_class_body_keeps_its_scope(body: Option<Node<'_>>) -> bool {
    let stmts: Vec<Node<'_>> = match body {
        None => return true,
        Some(n) => match n.as_statements_node() {
            Some(s) => s.body().iter().collect(),
            None => vec![n],
        },
    };
    !stmts.iter().any(writes_a_local)
}

/// Whether a constant of this name is assigned a value that MINTS a class at
/// RUNTIME (`Data.define`, `Struct.new`, `Class.new`) -- the only shapes a bare
/// `class Name ... end` should REOPEN (via `class_eval`) rather than define
/// fresh. Stricter than [`const_is_assigned`]: an ordinary value constant
/// (`C = 7`) is ignored, so a same-named constant in an UNRELATED lexical scope
/// (a bare `C = 7` inside `module B`, which lowers to a scope-less `ConstWrite`)
/// does not misroute a fresh nested `class C` onto the runtime-reopen path.
pub(crate) fn const_holds_runtime_class(hir: &Hir, name: &str) -> bool {
    hir.const_write_values_in_scope(name)
        .iter()
        .any(|&v| value_mints_runtime_class(hir, v))
}

/// The member list of a `Struct.new(:a, :b)` zeo can compile to a REAL class,
/// or `None` to leave it on the runtime path.
///
/// Deliberately narrow, and everything it rejects keeps today's complete
/// `rstruct::struct_new` fallback. A compiled struct is an ordinary generated
/// class whose members are hidden slots, so an accessor reaches a field
/// instead of a dynamic send into an overlay closure. What it will not take:
///
/// - `Data.define` (readers only, frozen on construct, `#with`),
/// - `keyword_init:` or any other keyword, and any block body,
/// - a computed or non-symbol member,
/// - a member whose name is not a plain lowercase identifier
///   (`Struct.new(:verbose?)`), or that is a Ruby KEYWORD (`Struct.new(:class)`,
///   which is legal and even shadows `Kernel#class`) -- the synthesized source
///   below spells members as accessor `def` names and `@name` ivars, and
///   neither can be either of those.
///
/// `ZEO_DEBUG=runtime-struct` turns the whole thing off.
/// Legal `Struct` member names that cannot be spelled as a parameter or
/// `def` name in the synthesized source. `Struct.new(:class)` is real code
/// -- it even shadows `Kernel#class`, which `issue_2975.rb` pins.
const RUBY_KEYWORDS: &[&str] = &[
    "alias", "and", "begin", "break", "case", "class", "def", "defined", "do", "else", "elsif",
    "end", "ensure", "false", "for", "if", "in", "module", "next", "nil", "not", "or", "redo",
    "rescue", "retry", "return", "self", "super", "then", "true", "undef", "unless", "until",
    "when", "while", "yield",
];

pub(crate) fn as_compiled_struct(value: &Node<'_>) -> Option<Vec<String>> {
    if crate::debug_flags::debug(crate::debug_flags::DebugFlag::RuntimeStruct) {
        return None;
    }
    let call = value.as_call_node()?;
    if String::from_utf8_lossy(call.name().as_slice()) != "new" || call.block().is_some() {
        return None;
    }
    let recv = call.receiver()?;
    if String::from_utf8_lossy(recv.as_constant_read_node()?.name().as_slice()) != "Struct" {
        return None;
    }
    let mut members = Vec::new();
    for arg in call.arguments()?.arguments().iter() {
        let name = String::from_utf8_lossy(arg.as_symbol_node()?.unescaped()).into_owned();
        let mut chars = name.chars();
        let plain = chars
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !plain || RUBY_KEYWORDS.contains(&name.as_str()) || members.contains(&name) {
            return None;
        }
        members.push(name);
    }
    (!members.is_empty()).then_some(members)
}

/// `NAME = Struct.new(:a, :b)` as an ordinary `class NAME < Struct` with an
/// accessor per member -- so the member is a field, and accessor
/// devirtualization applies to reading it.
///
/// The class body is SOURCE handed to `parse_and_lower_into`, the same route
/// `synthesize_ffi_struct` takes, rather than hand-built HIR: the accessors,
/// the constructor and its nil-filling defaults are exactly what an ordinary
/// `attr_accessor` and `def initialize` already lower to, and going through
/// the parser is what keeps them that way.
///
/// The `@member` slots are recorded as HIDDEN, which is what makes
/// `instance_variables` answer `[]` and `instance_variable_get(:@a)` answer
/// `nil`, as CRuby does. Everything else in the protocol -- `to_a`, `[]`, `==`,
/// `each`, `dig`, `inspect`, `Marshal` -- comes from `Struct`'s own shared
/// table, reached by MRO, and works unchanged because it asks for members by
/// INDEX.
pub(crate) fn synthesize_struct_class(
    hir: &mut Hir,
    name: &str,
    members: &[String],
) -> PResult<NodeId> {
    let accessors = members
        .iter()
        .map(|m| format!(":{m}"))
        .collect::<Vec<_>>()
        .join(", ");
    // NO synthesized `initialize`. `Struct#initialize` is a native row over
    // `rstruct::bind_members` -- the one semantic kernel -- and the class
    // inherits it.
    //
    // Not spelled here as Ruby, whatever a compiled body would gain: such a
    // body counts its own arguments with `args.size` and reaches nine more
    // user-visible sends besides, so ANY program that patches one of them
    // breaks every Struct and Data construction: `module M; def size = super
    // * 10; end; class Array; prepend M; end` makes `Struct.new(:a).new(1)`
    // raise `struct size differs`. CRuby's `rb_struct_initialize` is C and
    // reads `argc`, which no monkeypatch can reach.
    // The `def` is still needed: a compiled class's method table is FLATTENED
    // from ancestors that carry a user `Scope`, and `Struct#initialize` is a
    // native row -- an omitted `initialize` resolved all the way to
    // `BasicObject#initialize`, which refused its arguments.
    //
    // The body is one call into the runtime kernel, with the call SHAPE
    // already split by the calling convention. A `super` would not do: the
    // native `initialize` reads the shape off a kw-marked trailing hash, and
    // the mark does not survive the fused `Klass.new` path.
    let src = format!(
        "class {name} < Struct\n  attr_accessor {accessors}\n  \
         def initialize(*args, **kw)\n    __zeo_struct_init(args, kw)\n  end\nend\n"
    );
    // The offsets `parse_and_lower_into` produces index `src`, not the file
    // being lowered, so the class would claim a position it never occupied --
    // one that reads as EARLIER than everything above it. Re-stamp it with the
    // `NAME = Struct.new(...)` the user actually wrote, which is where ruby
    // reports the class as declared and where `const_added` announces it.
    let written_at = hir.current_span();
    let nodes = parse_and_lower_into(hir, &src)?;
    let [class_def] = nodes[..] else {
        return Err(crate::diagnostics::lower::LowerError::syntax(
            "a synthesized struct class must lower to exactly one ClassDef",
        ));
    };
    hir.set_span(class_def, written_at);
    // A member is not an instance variable: `@a` in a method of the struct
    // is its own ivar, and reads nil until assigned. The accessors reach the
    // member through a slot named with a NUL first, which no `@name` can
    // spell; `clif::classes` strips it back off for the member list.
    let slots: Vec<String> = members.iter().map(|m| format!("\0{m}")).collect();
    hir.struct_members.insert(class_def, slots);
    name_struct_writer_parameters(hir, class_def);
    hide_member_slots(hir, class_def, members);
    Ok(class_def)
}

/// Renames the accessors' `@member` reads and writes to the member slots
/// [`synthesize_struct_class`] names.
fn hide_member_slots(hir: &mut Hir, class_def: NodeId, members: &[String]) {
    let HirNode::ClassDef { body, .. } = hir[class_def].clone() else {
        return;
    };
    for stmt in body {
        let HirNode::DefMethod { body, .. } = hir[stmt].clone() else {
            continue;
        };
        for node in body {
            match &mut hir[node] {
                HirNode::IvarRead(name) | HirNode::IvarWrite(name, _) if members.contains(name) => {
                    *name = format!("\0{name}");
                }
                _ => {}
            }
        }
    }
}

/// Ruby names a Struct WRITER's parameter `_`, where an `attr_accessor`
/// writer names nothing. Both spell the same generated accessor here, so the
/// struct half renames its slot after lowering -- which keeps the
/// `ATTR_GENERATED` mark, and with it the frame elision on every struct write.
fn name_struct_writer_parameters(hir: &mut Hir, class_def: NodeId) {
    let HirNode::ClassDef { body, .. } = hir[class_def].clone() else {
        return;
    };
    for stmt in body {
        let HirNode::DefMethod { name, params, .. } = &hir[stmt] else {
            continue;
        };
        if !name.ends_with('=') || params.required.len() != 1 {
            continue;
        }
        let old = params.required[0].clone();
        let HirNode::DefMethod { params, body, .. } = &mut hir[stmt] else {
            continue;
        };
        params.required[0] = "_".to_string();
        let body = body.clone();
        for node in body {
            if let HirNode::IvarWrite(_, value) = hir[node].clone()
                && let HirNode::LocalRead(read) = &mut hir[value]
                && *read == old
            {
                *read = "_".to_string();
            }
        }
    }
}

/// `Name = Module.new` -- the block body, or `None` for the bodyless form --
/// when the module that call mints is, observably, the module
/// `module Name ... end` would define. `None` leaves it on the runtime path.
///
/// The two spellings are NOT interchangeable in general, and the whole
/// difference is the cref. `Module.new`'s block opens none: `Module.nesting`
/// answers `[]` inside it, `X = 1` writes `Object::X`, a nested `class Inner`
/// defines `Object::Inner`, and a `def` written there looks constants up from
/// the ENCLOSING scope -- so `Module.new { include Wrap; def r = FROM_WRAP }`
/// raises NameError where the keyword form answers. A `module` body opens a
/// cref, and every one of those reads differently.
///
/// Every way that difference shows is a CONSTANT, so the accepted set is the
/// bodies that name none: `def`, `attr_*`, a visibility directive, `alias`,
/// `alias_method` and `define_method`, and nothing else. Under that
/// restriction the two spellings define the same module method for method,
/// which is what lets a later `include Name` be a static MRO edge instead of a
/// runtime splice no compiled ancestry can see.
///
/// The bodyless `Readers = Module.new` (rspec-core writes exactly that) is the
/// degenerate case: no body, so nothing to observe a cref with at all.
pub(crate) fn as_synthesized_module<'pr>(value: &Node<'pr>) -> Option<Option<Node<'pr>>> {
    let call = value.as_call_node()?;
    if String::from_utf8_lossy(call.name().as_slice()) != "new" || call.arguments().is_some() {
        return None;
    }
    let recv = call.receiver()?;
    if constant_path_name(&recv).ok()?.trim_start_matches("::") != "Module" {
        return None;
    }
    let Some(block) = call.block() else {
        return Some(None);
    };
    // `Module.new(&builder)` passes a proc whose body zeo cannot see here, and
    // `Module.new { |m| ... }` binds the module to a parameter -- neither is a
    // body this can read.
    let block = block.as_block_node()?;
    if block.parameters().is_some() {
        return None;
    }
    module_body_is_definitions_only(block.body()).then(|| block.body())
}

/// The statement whitelist [`as_synthesized_module`] accepts: definitions, and
/// the directives that only name a method. A `def`'s own body is NOT checked
/// here -- it can still read a constant, which the caller rejects after
/// lowering, where the arena makes the question exact.
fn module_body_is_definitions_only(body: Option<Node<'_>>) -> bool {
    let stmts: Vec<Node<'_>> = match body {
        None => return true,
        Some(n) => match n.as_statements_node() {
            Some(s) => s.body().iter().collect(),
            None => vec![n],
        },
    };
    stmts.iter().all(|stmt| {
        if stmt.as_def_node().is_some() || stmt.as_alias_method_node().is_some() {
            return true;
        }
        // `attr_reader :x` / `private` and friends are receiverless calls, not
        // node kinds of their own. `include`/`extend`/`prepend` are deliberately
        // absent: an ancestor joins a real module's constant lookup and joins
        // nothing at all in a block.
        let Some(call) = stmt.as_call_node() else {
            return false;
        };
        if call.receiver().is_some() {
            return false;
        }
        matches!(
            String::from_utf8_lossy(call.name().as_slice()).as_ref(),
            "attr_reader"
                | "attr_writer"
                | "attr_accessor"
                | "private"
                | "public"
                | "protected"
                | "module_function"
                | "alias_method"
                | "define_method"
        )
    })
}

/// `Name = Module.new { <definitions> }` as the `module Name ... end` it is
/// equivalent to -- see [`as_synthesized_module`] for why the equivalence holds
/// only for a body that names no constant.
///
/// The body is lowered before that last condition can be checked: whether a
/// `def` in it reads a constant is a question about its whole subtree, and the
/// arena answers it exactly where a prism walk would have to re-derive it. A
/// rejected lowering leaves its nodes unreferenced, which costs only arena
/// space -- the value is lowered again as an ordinary block, and the three
/// whole-arena scans that exist all key on `ConstWrite`, which this body cannot
/// contain.
pub(crate) fn synthesize_module(
    result: &ParseResult,
    hir: &mut Hir,
    name: &str,
    body: Option<Node<'_>>,
) -> PResult<Option<NodeId>> {
    let lowered = lower_class_body(result, hir, body, None, Some(name))?;
    if lowered.iter().any(|&id| names_a_constant(hir, id)) {
        return Ok(None);
    }
    let def = hir.push(HirNode::ClassDef {
        name: name.to_string(),
        superclass: None,
        body: lowered,
        is_module: true,
    });
    // The `module` keyword answers with its body's last statement; the
    // assignment this replaces answers with the module. They are the same
    // statement only when the value is discarded, so name the module again.
    let value = hir.push(HirNode::ClassRef(name.to_string()));
    Ok(Some(hir.push(HirNode::Seq(vec![def, value]))))
}

/// Whether this subtree names a constant anywhere -- including as the receiver
/// of `Module.nesting`, the one construct that reads the cref itself rather
/// than a name through it.
fn names_a_constant(hir: &Hir, id: NodeId) -> bool {
    let names = matches!(
        hir[id],
        HirNode::ClassRef(_)
            | HirNode::New { .. }
            | HirNode::ConstWrite { .. }
            | HirNode::DynConstRead { .. }
            | HirNode::DynConstWrite { .. }
            | HirNode::ClassDef { .. }
            | HirNode::Include(_)
            | HirNode::Extend(_)
            | HirNode::Prepend(_)
            | HirNode::ClassMethodPrepend(_)
            | HirNode::ConstantVisibility { .. }
            | HirNode::Using(_)
            | HirNode::Refine { .. }
    );
    if names {
        return true;
    }
    // `for_each_child` rather than a hand-rolled walk, for the reason
    // `rescope_body_constants` gives: it is the exhaustive one, and a missed
    // variant here is a silent divergence rather than a compile error.
    let mut found = false;
    hir[id].for_each_child(&mut |c| {
        if !found {
            found = names_a_constant(hir, c);
        }
    });
    found
}

/// Whether `value` is a `Data.define(...)` / `Struct.new(...)` / `Class.new(...)`
/// call -- an expression that produces a class object at runtime.
fn value_mints_runtime_class(hir: &Hir, value: NodeId) -> bool {
    let HirNode::Call {
        receiver: Some(r),
        name,
        ..
    } = &hir[value]
    else {
        return false;
    };
    let HirNode::ClassRef(recv) = &hir[*r] else {
        return false;
    };
    let recv = recv.strip_prefix("::").unwrap_or(recv);
    matches!(
        (recv, name.as_str()),
        ("Data", "define") | ("Struct", "new") | ("Class", "new") | ("Module", "new")
    )
}

/// Whether an already-lowered `class`/`module` DEFINES this name HERE, making
/// it a compile-time class even if some later statement also assigns the
/// constant.
///
/// "Here" is the whole point: the question is asked of a name as one site
/// spells it, and an arena-wide scan for a `ClassDef` of that name answers for
/// every OTHER site too. citrus writes `module Citrus; class Error <
/// StandardError`, toml-rb writes `module TomlRB; Error =
/// Class.new(StandardError); class ValueOverwriteError < Error` -- and with
/// both compiled in, the scan said toml-rb's `Error` names a compile-time
/// class, which put its subclass on the static path where nothing defines
/// `TomlRB::Error` at all.
pub(crate) fn const_is_class_def(hir: &Hir, name: &str) -> bool {
    hir.class_defined_in_scope(name)
}

/// A class body's statement list -- like `lower_statement_list`, but
/// recognizes a handful of zero-receiver call shapes at this exact position
/// (mirroring `lower_node`'s own `define_method`/`loop` desugars) that a
/// strict 1-statement-to-1-node map can't express: `attr_reader`/
/// `attr_writer`/`attr_accessor` each expand into MULTIPLE synthesized
/// `DefMethod`s from one statement, and `private`/`public`/`protected` expand
/// into NONE.
/// `class Name < <expression>` -- a subclass of a class that does not exist
/// until run time (`Struct.new(:x, :y)`, `Class.new`, a class held in a
/// variable). `analyze::register_class` can only link a subclass to a parent
/// it already knows statically, so this desugars to the runtime form zeo
/// already supports end to end: `Name = Class.new(<expression>) { <body> }`.
///
/// The one place the desugar is NOT a faithful rewrite is local-variable
/// scope: a class body opens a FRESH scope, while the block body it becomes
/// closes over the enclosing one, so `y = 1` in the body would assign the
/// caller's `y` instead of a new one. A `def` in the body is unaffected (its
/// own body already resolves in a fresh scope -- verified against the
/// oracle), so only a direct local write is rejected, rather than left to
/// diverge silently.
pub(crate) fn lower_runtime_class(
    result: &ParseResult,
    hir: &mut Hir,
    name: &str,
    superclass: Option<&Node<'_>>,
    body: Option<Node<'_>>,
) -> PResult<NodeId> {
    let parent = match superclass {
        Some(sc) => lower_node(result, hir, sc)?,
        // No `< Super` clause -- reached only by the runtime-SCOPE route
        // below, where the class itself is ordinary but its namespace is a
        // constant no compile-time class backs.
        None => hir.push(HirNode::ClassRef("Object".to_string())),
    };
    let block = lower_runtime_class_body(result, hir, name, Some(name), body)?;
    let class_class = hir.push(HirNode::ClassRef("Class".to_string()));
    let new_call = hir.push(HirNode::Call {
        receiver: Some(class_class),
        name: "new".to_string(),
        args: vec![ArrayElem::Single(parent)],
        kwargs: Vec::new(),
        block: Some(block),
        block_arg: None,
        safe: false,
    });
    // `class NS::Item < ...` has to write `Item` INSIDE `NS`, not a flat
    // constant that happens to be spelled `"NS::Item"` -- the latter reads
    // back only through the identical spelling, and leaves `NS.constants`
    // empty.
    let path = crate::constpath::ConstPath::parse(name);
    Ok(hir.push(HirNode::ConstWrite {
        scope: path.scope().map(str::to_string),
        name: path.base().to_string(),
        value: new_call,
    }))
}

/// `module NS::Inner` under a runtime-minted `NS` -- the module twin of
/// [`lower_runtime_class`], writing `Module.new { body }` instead.
pub(super) fn lower_runtime_module(
    result: &ParseResult,
    hir: &mut Hir,
    name: &str,
    body: Option<Node<'_>>,
) -> PResult<NodeId> {
    let block = lower_runtime_class_body(result, hir, name, Some(name), body)?;
    let module_class = hir.push(HirNode::ClassRef("Module".to_string()));
    let new_call = hir.push(HirNode::Call {
        receiver: Some(module_class),
        name: "new".to_string(),
        args: Vec::new(),
        kwargs: Vec::new(),
        block: Some(block),
        block_arg: None,
        safe: false,
    });
    let path = crate::constpath::ConstPath::parse(name);
    Ok(hir.push(HirNode::ConstWrite {
        scope: path.scope().map(str::to_string),
        name: path.base().to_string(),
        value: new_call,
    }))
}

/// Whether a definition's NAMESPACE is a constant that mints its class at
/// runtime -- `class SecretKeys::Encryptor` under `class SecretKeys <
/// DelegateClass(Hash)`, or `class Kanshi::Collector` under `Kanshi =
/// Class.new`.
///
/// Only the IMMEDIATE prefix is asked, and only when nothing also `class`-
/// defines it: a namespace some other statement opens statically stays on the
/// static path, where nesting, `include` and visibility all still work.
pub(super) fn runtime_scoped_definition(hir: &Hir, name: &str) -> bool {
    let Some(prefix) = crate::constpath::ConstPath::parse(name).scope() else {
        return false;
    };
    const_holds_runtime_class(hir, prefix) && !const_is_class_def(hir, prefix)
}

/// `class D ... end` REOPENING a constant that holds a runtime class
/// (`D = Data.define(:x)`) -- lowered to `D.class_eval { <body> }`, which
/// installs onto the existing class. The static path would instead register a
/// brand-new, memberless class `D`, so a generated `Data`/`Struct` reader
/// could not resolve inside the reopened body.
pub(crate) fn lower_runtime_class_reopen(
    result: &ParseResult,
    hir: &mut Hir,
    name: &str,
    body: Option<Node<'_>>,
) -> PResult<NodeId> {
    let block = lower_runtime_class_body(result, hir, name, Some(name), body)?;
    let target = hir.push(HirNode::ClassRef(name.to_string()));
    Ok(hir.push(HirNode::Call {
        receiver: Some(target),
        name: "class_eval".to_string(),
        args: Vec::new(),
        kwargs: Vec::new(),
        block: Some(block),
        block_arg: None,
        safe: false,
    }))
}

/// The `(namespace expression, leaf)` of a definition whose NAMESPACE is a
/// runtime VALUE rather than a constant path -- `None` for every ordinary
/// name, which is what keeps `Foo::Bar` on the static path.
///
/// Four shapes in the corpus, all the same thing said differently:
///
/// - `class self::Task`, written inside a hook block (`included do ... end`),
///   where `self` is whichever class is being extended -- dk-dumpdb gives
///   every including script its own `Task` subclass this way;
/// - `module Wires.current_network::Namespace` and
///   `class parent::Pagination` (a local), where the namespace is computed;
/// - `module Num[16]::Trigonometry`, an index;
/// - JRuby's lowercase java packages, `class org::jrubyparser::ast::CallNode`
///   -- `org::jrubyparser` parses as a CALL, not a constant path, so this is
///   the same shape. CRuby compiles that file and raises `NameError` on `org`
///   when the definition runs, which is exactly what lowering the namespace as
///   an expression produces.
pub(super) fn runtime_scoped_definition_name<'a>(path: &Node<'a>) -> Option<(Node<'a>, String)> {
    let cp = path.as_constant_path_node()?;
    let parent = cp.parent()?;
    // `Foo::Bar` / `A::B::C` name a compile-time namespace; only a namespace
    // no constant path can spell belongs here.
    if parent.as_constant_read_node().is_some() || parent.as_constant_path_node().is_some() {
        return None;
    }
    Some((
        parent,
        String::from_utf8_lossy(cp.name()?.as_slice()).into_owned(),
    ))
}

/// `class <expr>::Task < Super ... end` -> `<expr>::Task = Class.new(Super) {
/// body }`, and the `module` half -> `Module.new { body }`. The runtime
/// namespace is what makes the static path impossible: the constant lands on
/// whatever the expression answers when the definition RUNS, so there is no
/// compile-time class to register. Same desugar the runtime-superclass form
/// takes (`lower_runtime_class`), and the same body treatment with it.
pub(super) fn lower_scoped_definition(
    result: &ParseResult,
    hir: &mut Hir,
    scope: NodeId,
    leaf: String,
    body: Option<Node<'_>>,
    superclass: Option<NodeId>,
) -> PResult<NodeId> {
    let block = lower_runtime_class_body(result, hir, &leaf, None, body)?;
    let (builder, args) = match superclass {
        Some(parent) => ("Class", vec![ArrayElem::Single(parent)]),
        None => ("Module", Vec::new()),
    };
    let builder_ref = hir.push(HirNode::ClassRef(builder.to_string()));
    let value = hir.push(HirNode::Call {
        receiver: Some(builder_ref),
        name: "new".to_string(),
        args,
        kwargs: Vec::new(),
        block: Some(block),
        block_arg: None,
        safe: false,
    });
    Ok(hir.push(HirNode::DynConstWrite {
        scope,
        name: leaf,
        value,
    }))
}

/// The shared body half of the two runtime-class desugars: lowers the class
/// body and wraps it as the block those forms pass. See `lower_runtime_class`
/// for why a local write in the body is rejected rather than diverging.
fn lower_runtime_class_body(
    result: &ParseResult,
    hir: &mut Hir,
    name: &str,
    cref: Option<&str>,
    body: Option<Node<'_>>,
) -> PResult<NodeId> {
    // A runtime class body EMITS as an ordinary block, but it IS a class body
    // in ruby, so it opens a cref. That is what makes `@@v` legal here rather
    // than "class variable access from toplevel" -- and it puts the body's
    // cvars on the same storage the `def`s in it resolve against, which a
    // reflective `class_variable_set` on the built class would NOT have done
    // (two tables, and `Adapter.stopping?` and
    // `Adapter.class_variable_get(:@@stopping)` then disagreed).
    let body = lower_class_body(result, hir, body, None, Some(name))?;
    // ... and a block SHARES the enclosing local scope, where a class body has
    // its own. Renaming what the body assigns restores that: sidekiq's adapter
    // binds a `callback` lambda in the body and reads it from two nested
    // blocks, which would otherwise have collided with (or been shadowed by) an
    // enclosing `callback`. See `rename::isolate_runtime_class_locals`.
    let seq = hir.nodes().len();
    crate::rename::isolate_runtime_class_locals(hir, &body, seq);
    // The body runs as a BLOCK with the new class as `self`, so a statement
    // that only the static class path can emit (`include`, a visibility
    // directive, `alias`, a nested class) would reach codegen's "top-level-only
    // node in expression position" panic. Rewrite each into its runtime
    // spelling -- a self-send the runtime class receiver serves -- so the class
    // builds at runtime. See `transform_runtime_class_body`.
    let body = transform_runtime_class_body(hir, body)?;
    // The own-name rewrite applies only where the bare name really does name
    // THIS class: a static constant path (`class Token` in `module JMESPath`,
    // or a reopen). A runtime-SCOPED definition passes `cref: None`, and there
    // the leaf names nothing reachable -- `class self::Task` writes onto
    // whichever object `self` is, so a bare `Task` in its body is the
    // TOP-LEVEL `Task`, which is the module the body then includes. Rewriting
    // that to `self` turned `include Task` into `include self`.
    let own = cref.is_some().then_some(name);
    rescope_body_constants(hir, cref, own, &body)?;
    Ok(hir.push(HirNode::Block {
        params: Box::default(),
        body,
    }))
}

/// Re-points a READ of a constant this body defines at the class the body is
/// building -- the other half of `transform_runtime_class_body`'s `Const`
/// rewrite.
///
/// A bare constant lowers to a `ClassRef` that codegen resolves against the
/// LEXICALLY-enclosing class, and a runtime class body is a block whose
/// enclosing class is whatever surrounds it -- `Object` at the top level. The
/// write moved to the built class, so `Kw::KW` answers from outside while
/// `def read = KW` still looked on Object and raised `uninitialized constant`.
/// Before the write moved, both agreed on Object: wrong, but consistent.
///
/// The two positions need different scopes, because they run under different
/// `self`:
///
///   IN THE BODY, `self` IS the class, so `SelfRef` is exact -- and it is the
///   only correct answer, since the constant holding the class is not assigned
///   until the whole `Name = Class.new(...) { body }` expression finishes.
///
///   IN A `def`, `self` is the receiver, so the class is named through the
///   constant that holds it. That constant IS bound by the time any such
///   method can run.
///
/// Only the `class` KEYWORD spellings reach here (`class Name < <expr>`, a
/// reopen, and `class <expr>::Name`), and all open a real cref, which is what
/// makes this ruby's answer rather than a guess. `Class.new do NAME = v end`
/// is a plain block: its cref is the enclosing one, so ruby writes
/// `Object::NAME` there and no rewrite is owed.
///
/// `cref: None` is the one shape with no answer for the DEF position: a
/// runtime-SCOPED definition (`module <expr>::Ns`) is reachable only through
/// the namespace expression, which cannot be re-evaluated at each read. A
/// body-defined constant read from inside a `def` is refused there rather than
/// resolved against the wrong scope -- the body position still works, since
/// `self` is the class being built.
fn rescope_body_constants(
    hir: &mut Hir,
    cref: Option<&str>,
    own: Option<&str>,
    body: &[NodeId],
) -> PResult<()> {
    // What the body defines, as `transform_runtime_class_body` left it: a
    // `NAME = value` and a nested `class Inner` both become a `DynConstWrite`
    // against the body's `self`. A hand-written `self::NAME = v` is the same
    // statement said out loud, and belongs in the set for the same reason.
    fn owns(hir: &Hir, id: NodeId) -> Option<String> {
        match &hir[id] {
            HirNode::DynConstWrite { scope, name, .. }
                if matches!(hir[*scope], HirNode::SelfRef) =>
            {
                Some(name.clone())
            }
            _ => None,
        }
    }
    // `for_each_child` rather than a hand-rolled walk: it is the exhaustive
    // one, and a missed variant here is a silently unresolved constant.
    fn walk(hir: &Hir, id: NodeId, in_def: bool, out: &mut Vec<(NodeId, bool)>) {
        out.push((id, in_def));
        let in_def = in_def || matches!(hir[id], HirNode::DefMethod { .. });
        hir[id].for_each_child(&mut |c| walk(hir, c, in_def, out));
    }

    let mut reachable = Vec::new();
    for &id in body {
        walk(hir, id, false, &mut reachable);
    }
    let defined: std::collections::HashSet<String> = reachable
        .iter()
        .filter_map(|&(id, _)| owns(hir, id))
        .collect();
    if defined.is_empty() && own.is_none() {
        return Ok(());
    }
    for (id, in_def) in reachable {
        // A bare constant is a `ClassRef`, but `Inner.new(...)` keeps its own
        // `New` node with the class as a plain string -- a nested `class Inner`
        // is read that way far more often than as a bare value, so both spell
        // the same rewrite.
        let name = match &hir[id] {
            HirNode::ClassRef(n) | HirNode::New { class_name: n, .. } => n.clone(),
            _ => continue,
        };
        // The class's OWN name is the same problem one step out. Ruby binds the
        // constant BEFORE running the body, so `class Token < Struct.new(...)`
        // may say `Token.new` in its own body -- jmespath's does, and every
        // `Struct.new` subclass that names itself. zeo assigns it only when the
        // whole `Name = Class.new(...) { body }` expression finishes, so the
        // read raised `uninitialized constant`. In the body `self` IS the class,
        // which is exact; inside a `def` the constant is bound by the time the
        // method can run, so that read is left to the ordinary lexical path.
        let is_own = own.is_some_and(|o| o == name);
        if is_own && in_def {
            continue;
        }
        if !is_own && !defined.contains(&name) {
            continue;
        }
        // The own name IS `self` here, not a constant living ON self: the body
        // of `class T < ...` reads `T` as the class being built. A body-defined
        // constant is the other shape -- it really is stored on the class, so
        // it stays a scoped read.
        let read = if is_own {
            HirNode::SelfRef
        } else {
            let scope = if in_def {
                let Some(cref) = cref else {
                    return Err(format!(
                        "`{name}` is defined in a class/module body whose NAMESPACE is a runtime \
                         value, and read from a `def` inside it -- no constant path names the \
                         class, so the read has no scope to resolve against (zeo limitation). \
                         Move the constant outside the definition, or name the namespace."
                    )
                    .into());
                };
                hir.push(HirNode::ClassRef(cref.to_string()))
            } else {
                hir.push(HirNode::SelfRef)
            };
            HirNode::DynConstRead {
                scope,
                name,
                lenient: false,
            }
        };
        // Taken out of the arena rather than cloned: a `New`'s arguments move
        // straight into the `Call` that replaces it, and `KwArg` is not `Clone`.
        hir[id] = match std::mem::replace(&mut hir[id], HirNode::NilLit) {
            HirNode::New {
                args,
                kwargs,
                block,
                ..
            } => {
                let receiver = hir.push(read);
                HirNode::Call {
                    receiver: Some(receiver),
                    name: "new".to_string(),
                    args: args.into_iter().map(ArrayElem::Single).collect(),
                    kwargs,
                    block,
                    block_arg: None,
                    safe: false,
                }
            }
            _ => read,
        };
    }
    Ok(())
}

/// Rewrites the static-only nodes a lowered class body can hold into the
/// runtime self-sends a `Class.new { ... }`/`class_eval { ... }` block serves,
/// so a class with a DYNAMIC superclass (`class Tempfile < DelegateClass(File)`)
/// or a runtime reopen can carry the same bodies a statically-registered class
/// can. `def`/`ConstWrite`/`class << self`'s class-method `def`s pass through
/// untouched (codegen already emits those in block position). A nested class/
/// module is desugared to a runtime `Const = Class.new(Super) { body }` (its
/// body transformed the same way, recursively).
///
/// This is the COMPILE path: the rewritten sends resolve at runtime through the
/// class/module builtins (`include`/`alias_method`/visibility/...). A construct
/// with no runtime builtin still lowers (it becomes a self-send that raises
/// NoMethodError only if actually executed) -- acceptable for a runtime class
/// on an otherwise-unreachable path, and never worse than the previous hard
/// compile error.
pub(super) fn transform_runtime_class_body(
    hir: &mut Hir,
    body: Vec<NodeId>,
) -> PResult<Vec<NodeId>> {
    // Classify without holding the `&hir[id]` borrow across the node-building
    // mutations below (each rewrite pushes fresh nodes).
    enum Rewrite {
        Nested(String, Option<String>, Vec<NodeId>, bool),
        Cond(NodeId, Vec<NodeId>, Vec<NodeId>),
        Directive,
        /// Keep the node, and follow it with the visibility send its `def`
        /// absorbed at lowering time. The `bool` is `is_class_method`: a
        /// `def self.x` is marked on the SINGLETON, like every other
        /// class-method directive.
        KeepAndScope(String, Visibility, bool),
        /// A bare `NAME = value`, re-pointed at the class this body is
        /// building.
        Const(String, NodeId),
        Keep,
    }
    let mut out = Vec::with_capacity(body.len());
    for id in body {
        let rewrite = match &hir[id] {
            HirNode::ClassDef {
                name,
                superclass,
                body,
                is_module,
            } => Rewrite::Nested(name.clone(), superclass.clone(), body.clone(), *is_module),
            // A conditional guarding definitions (`if RUBY_VERSION < "3.2";
            // module PathAttr; ...; end`, tempfile): transform each branch the
            // same way and KEEP the runtime `if`, so the conditional still
            // decides at runtime which definitions execute.
            HirNode::If {
                cond,
                then_body,
                else_body,
            } => Rewrite::Cond(*cond, then_body.clone(), else_body.clone()),
            node if node.is_class_body_directive() => Rewrite::Directive,
            // A bare `NAME = value` resolves its OWNER from the enclosing
            // cref at compile time, and a runtime class body is emitted as a
            // block, whose cref is whatever encloses it -- `Object` at the top
            // level. So `class Sub < expr; OPEN = 1; end` wrote `Object::OPEN`
            // and `Sub::OPEN` was a NameError. Re-point it at the class the
            // body is building, which is the block's `self`.
            //
            // An explicit `Foo::BAR = v` passes through: it names its own
            // owner and never meant this one.
            HirNode::ConstWrite {
                scope: None,
                name,
                value,
            } => Rewrite::Const(name.clone(), *value),
            // `private :m` naming a method the SAME body defines is retagged
            // onto the `def` at lowering time, so no `MethodVisibility` node
            // survives for the directive rewrite to find. On the static path
            // that is enough -- the visibility rides the `Scope` into the
            // emitted dispatch row. Here the `def` becomes a RUNTIME
            // definition, which carries no visibility of its own, so the mark
            // has to be re-spoken as the send the class body serves.
            //
            // Only the same-body case needs this. `private :inherited_name`
            // leaves a real `MethodVisibility` and rewrites like any other
            // directive.
            HirNode::DefMethod {
                name,
                visibility,
                is_class_method,
                ..
            } if *visibility != Visibility::Public => {
                Rewrite::KeepAndScope(name.clone(), *visibility, *is_class_method)
            }
            _ => Rewrite::Keep,
        };
        let node = match rewrite {
            Rewrite::Nested(name, superclass, inner, is_module) => runtime_nested_class(
                hir,
                name,
                superclass,
                inner,
                is_module,
                NestedTarget::OnSelf,
            )?,
            Rewrite::Cond(cond, then_body, else_body) => {
                let then_body = transform_runtime_class_body(hir, then_body)?;
                let else_body = transform_runtime_class_body(hir, else_body)?;
                hir.push(HirNode::If {
                    cond,
                    then_body,
                    else_body,
                })
            }
            // `is_class_body_directive` says this node cannot stand in block
            // position, so the shared table owes it a spelling. A `None` here
            // means the two have drifted, and codegen would report it far from
            // its cause ("top-level-only node in expression position").
            Rewrite::Directive => runtime_directive_spelling(hir, id)?.ok_or(
                "a class-body directive has no runtime spelling -- add it to \
                 `runtime_directive_spelling` alongside `is_class_body_directive`",
            )?,
            // The `def` runs first, then the mark -- ruby's own order, and the
            // only one that works: `private :m` names a method that has to
            // already exist.
            Rewrite::KeepAndScope(name, visibility, is_class_method) => {
                out.push(id);
                let vis = visibility_name(visibility);
                if is_class_method {
                    // `private`/`public`/`protected` are private methods of
                    // Module, so the singleton form goes through `send` -- the
                    // same spelling `runtime_directive_spelling` uses for a
                    // `ClassMethodVisibility` naming an inherited method.
                    let args = vec![sym_lit(hir, vis.to_string()), sym_lit(hir, name)];
                    runtime_singleton_send(hir, "send", args)
                } else {
                    let args = vec![sym_lit(hir, name)];
                    runtime_self_send(hir, vis, args)
                }
            }
            Rewrite::Const(name, value) => {
                let scope = hir.push(HirNode::SelfRef);
                hir.push(HirNode::DynConstWrite { scope, name, value })
            }
            Rewrite::Keep => id,
        };
        out.push(node);
    }
    Ok(out)
}

/// A receiver-less (implicit-`self`) runtime call node -- the class body block's
/// `self` is the runtime class, so this dispatches to its class/module builtin.
/// The directives inside a class-body `if` whose condition analyze could not
/// decide, rewritten to the runtime self-send the enclosing class body serves
/// (`self` there IS the class). Without this they reach codegen as directives in
/// EXPRESSION position -- "a definition-level construct used as a VALUE".
///
/// `def`s and nested `class`es are deliberately left alone: a conditional `def`
/// already has its own registration (`analyze::register_conditional_defs`) and
/// its own runtime `define_method` emission, and a nested class its own site.
/// Only the directives with no expression form of their own are rewritten.
///
/// ruby_parser closes with `if ENV["RP_LINENO_DEBUG"] then class RubyLexer;
/// alias old_lineno= lineno=; ...` -- a debug hook whose guard is a real
/// runtime question.
pub(crate) fn transform_conditional_class_body(hir: &mut Hir, body: &[NodeId]) -> Vec<NodeId> {
    let mut out = Vec::with_capacity(body.len());
    for &id in body {
        let nested = match &hir[id] {
            HirNode::If {
                cond,
                then_body,
                else_body,
            } => Some((*cond, then_body.clone(), else_body.clone())),
            _ => None,
        };
        out.push(match nested {
            Some((cond, then_body, else_body)) => {
                let then_body = transform_conditional_class_body(hir, &then_body);
                let else_body = transform_conditional_class_body(hir, &else_body);
                let rebuilt = hir.push(HirNode::If {
                    cond,
                    then_body,
                    else_body,
                });
                // The rewrite mints a NEW node, so a hoisted-guard mark on the
                // old one would be lost -- and losing it leaves the condition
                // inside the class-body function, reading locals that live
                // outside it.
                if hir.has_flag(id, crate::hir::NodeFlag::HOISTED_CLASS_GUARD) {
                    hir.set_flag(rebuilt, crate::hir::NodeFlag::HOISTED_CLASS_GUARD);
                }
                rebuilt
            }
            // A directive with no spelling stays put rather than failing the
            // compile: unlike the runtime-class path, the class here is real and
            // statically laid out, so a `refine` in the branch is analyze's to
            // answer, not this rewrite's.
            None => runtime_directive_spelling(hir, id)
                .ok()
                .flatten()
                .unwrap_or(id),
        });
    }
    out
}

/// The runtime spelling of one class-body DIRECTIVE: the self-send a block
/// whose `self` is the class serves, standing in for a layout the compiler
/// would otherwise have baked. `None` means the node already stands on its own
/// in block position (a `def`, a nested `class`, an ordinary statement), and
/// the caller decides what to do with it.
///
/// ONE table, read by both callers -- a class whose superclass is only known at
/// runtime ([`transform_runtime_class_body`]) and an undecidable class-body
/// `if` ([`transform_conditional_class_body`]). They kept two tables between
/// them, and both drifted from [`HirNode::is_class_body_directive`]: three gems
/// (danger, gitlab-labkit, activeadmin_settings_cached) reached codegen through
/// a directive neither had a row for.
pub(crate) fn runtime_directive_spelling(hir: &mut Hir, id: NodeId) -> PResult<Option<NodeId>> {
    /// Classified without holding the `&hir[id]` borrow across the node-building
    /// mutations below (each rewrite pushes fresh nodes).
    enum Rewrite {
        /// A send taking a module REFERENCE (`include M`).
        Mixin(&'static str, String),
        /// A send whose arguments are all symbols (`private :x`,
        /// `undef_method :a, :b`, `alias_method :new, :old`).
        Syms(&'static str, Vec<String>),
        /// Either of the above, but to the class's SINGLETON class -- the
        /// `class << self` half, where a class's own methods live.
        SingletonMixin(&'static str, String),
        SingletonSyms(&'static str, Vec<String>),
    }
    let rewrite = match &hir[id] {
        HirNode::Include(m) => Rewrite::Mixin("include", m.clone()),
        HirNode::Extend(m) => Rewrite::Mixin("extend", m.clone()),
        HirNode::Prepend(m) => Rewrite::Mixin("prepend", m.clone()),
        HirNode::ClassMethodPrepend(m) => Rewrite::SingletonMixin("prepend", m.clone()),
        HirNode::AliasMethod {
            new_name, old_name, ..
        } => Rewrite::Syms("alias_method", vec![new_name.clone(), old_name.clone()]),
        HirNode::MethodVisibility { name, visibility } => {
            Rewrite::Syms(visibility_name(*visibility), vec![name.clone()])
        }
        // `private`/`public`/`protected` are PRIVATE methods of Module, so the
        // singleton form has to go through `send` -- which is also the only
        // spelling that covers `protected`, ruby having no
        // `protected_class_method` to match its two siblings.
        HirNode::ClassMethodVisibility { name, visibility } => Rewrite::SingletonSyms(
            "send",
            vec![visibility_name(*visibility).to_string(), name.clone()],
        ),
        HirNode::ConstantVisibility { names, private } => Rewrite::Syms(
            if *private {
                "private_constant"
            } else {
                "public_constant"
            },
            names.clone(),
        ),
        HirNode::ModuleFunction(name) => Rewrite::Syms("module_function", vec![name.clone()]),
        HirNode::Undef(names) => Rewrite::Syms("undef_method", names.clone()),
        HirNode::ClassMethodUndef(names) => Rewrite::SingletonSyms("undef_method", names.clone()),
        // The one directive with no self-send that reproduces it: a refinement
        // is activated LEXICALLY by `using`, over the text that follows it, and
        // only the static path lays that out. A runtime `refine` send would
        // build the module and activate it nowhere.
        HirNode::Refine { target, .. } => {
            return Err(format!(
                "`refine {target}` inside a class built at runtime isn't supported (zeo \
                 limitation) -- a refinement activates lexically, which needs the enclosing \
                 class laid out at compile time"
            )
            .into());
        }
        _ => return Ok(None),
    };
    Ok(Some(match rewrite {
        Rewrite::Mixin(method, m) => {
            let arg = class_ref(hir, &m);
            runtime_self_send(hir, method, vec![arg])
        }
        Rewrite::SingletonMixin(method, m) => {
            let arg = class_ref(hir, &m);
            runtime_singleton_send(hir, method, vec![arg])
        }
        Rewrite::Syms(method, names) => {
            let args = names.into_iter().map(|n| sym_lit(hir, n)).collect();
            runtime_self_send(hir, method, args)
        }
        Rewrite::SingletonSyms(method, names) => {
            let args = names.into_iter().map(|n| sym_lit(hir, n)).collect();
            runtime_singleton_send(hir, method, args)
        }
    }))
}

fn runtime_self_send(hir: &mut Hir, name: &str, args: Vec<NodeId>) -> NodeId {
    hir.push(HirNode::Call {
        receiver: None,
        name: name.to_string(),
        args: args.into_iter().map(ArrayElem::Single).collect(),
        kwargs: Vec::new(),
        block: None,
        block_arg: None,
        safe: false,
    })
}

/// `singleton_class.<name>(args)` -- the same dispatch one level up. A class
/// body's `self` is the class, so its CLASS methods are its singleton class's
/// instance methods, which is where the `class << self` directives have to land.
fn runtime_singleton_send(hir: &mut Hir, name: &str, args: Vec<NodeId>) -> NodeId {
    let singleton = runtime_self_send(hir, "singleton_class", Vec::new());
    hir.push(HirNode::Call {
        receiver: Some(singleton),
        name: name.to_string(),
        args: args.into_iter().map(ArrayElem::Single).collect(),
        kwargs: Vec::new(),
        block: None,
        block_arg: None,
        safe: false,
    })
}

fn class_ref(hir: &mut Hir, name: &str) -> NodeId {
    hir.push(HirNode::ClassRef(name.to_string()))
}

fn sym_lit(hir: &mut Hir, name: String) -> NodeId {
    hir.push(HirNode::SymbolLit(name))
}

fn visibility_name(v: Visibility) -> &'static str {
    match v {
        Visibility::Private => "private",
        Visibility::Protected => "protected",
        Visibility::Public => "public",
    }
}

/// Where a nested class's const-assignment lands: on `self` (a runtime class
/// body, where `self` IS the class under construction) or on the enclosing
/// lexical scope (a `class << recv` body).
#[derive(PartialEq)]
pub(super) enum NestedTarget {
    OnSelf,
    Lexical,
}

/// A nested `class C < S; body; end` (or `module`) inside a runtime class body,
/// rebuilt as a runtime `C = Class.new(S) { body }` / `C = Module.new { body }`
/// const-assignment -- an ordinary expression that lives in block position. The
/// nested body is transformed the same way, so nesting composes.
pub(super) fn runtime_nested_class(
    hir: &mut Hir,
    name: String,
    superclass: Option<String>,
    body: Vec<NodeId>,
    is_module: bool,
    target: NestedTarget,
) -> PResult<NodeId> {
    let inner = transform_runtime_class_body(hir, body)?;
    let block = hir.push(HirNode::Block {
        params: Box::default(),
        body: inner,
    });
    let (builder, args) = if is_module {
        ("Module", Vec::new())
    } else {
        let parent = class_ref(hir, &superclass.unwrap_or_else(|| "Object".to_string()));
        ("Class", vec![ArrayElem::Single(parent)])
    };
    let builder_ref = hir.push(HirNode::ClassRef(builder.to_string()));
    let new_call = hir.push(HirNode::Call {
        receiver: Some(builder_ref),
        name: "new".to_string(),
        args,
        kwargs: Vec::new(),
        block: Some(block),
        block_arg: None,
        safe: false,
    });
    let path = crate::constpath::ConstPath::parse(&name);
    // An unscoped nested name belongs to whichever class this body is building
    // -- `class Sub < expr; class Inner; end; end` defines `Sub::Inner`. Only
    // the runtime-class-body caller can say so: inside a `class << obj` body
    // the same nesting still defines the constant LEXICALLY, on the enclosing
    // module, because a singleton opens no cref of its own.
    if target == NestedTarget::OnSelf && path.scope().is_none() {
        let scope = hir.push(HirNode::SelfRef);
        return Ok(hir.push(HirNode::DynConstWrite {
            scope,
            name: path.base().to_string(),
            value: new_call,
        }));
    }
    Ok(hir.push(HirNode::ConstWrite {
        scope: path.scope().map(str::to_string),
        name: path.base().to_string(),
        value: new_call,
    }))
}
