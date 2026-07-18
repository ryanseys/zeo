//! `Name = Struct.new(:a, :b)` -- compile-time class SYNTHESIS (Phase
//! 17.1-H). In this AOT model a Struct definition is recognized at
//! lowering time and rewritten into an ordinary `class Name < Struct`
//! whose accessors/initialize/members/==/each/... are generated as plain
//! Ruby source and parsed through the ordinary pipeline (the same trick
//! the exception prelude uses) -- everything downstream (analyze, MRO,
//! codegen, the registry) sees a perfectly ordinary class, monomorphized
//! for its exact member list. A `do ... end` block's method definitions
//! append AFTER the template's, so a user `def dist` overrides a template
//! method by the normal later-def-wins reopening rule.
//!
//! Rejections (clean compile errors): non-symbol members,
//! `Struct.new("Name", ...)`'s string form, a non-literal `keyword_init:`,
//! and -- enforced at the generic `.new` lowering in `parse::mod` --
//! `Struct.new` anywhere but a constant assignment's right-hand side.

use super::{lower_class_body_statement, lower_node, PResult};
use crate::hir::{Hir, HirNode, NodeId, Visibility};
use ruby_prism::Node;

/// Whether a synthesized value class is a mutable `Struct` (positional
/// nil-filling `new`, `Enumerable`, writers) or an immutable `Data`
/// (keyword `new`, no `each`, readers only + `with`).
#[derive(Clone, Copy, PartialEq)]
enum ValueKind {
    Struct,
    Data,
}

/// `Some(..)` when `value` is a `Struct.new(...)` call (the caller is
/// lowering `const_name = value`); `None` lets the ordinary ConstWrite
/// lowering proceed.
pub(super) fn try_lower_struct_def(
    result: &ruby_prism::ParseResult<'_>,
    hir: &mut Hir,
    const_name: &str,
    value: &Node<'_>,
) -> Option<PResult<NodeId>> {
    let call = value.as_call_node()?;
    let recv = call.receiver()?;
    let recv = recv.as_constant_read_node()?;
    if recv.name().as_slice() != b"Struct" || call.name().as_slice() != b"new" {
        return None;
    }
    Some(lower_value_def(ValueKind::Struct, result, hir, const_name, &call))
}

/// `Some(..)` when `value` is a `Data.define(...)` call -- the immutable
/// sibling of `Struct.new`.
pub(super) fn try_lower_data_def(
    result: &ruby_prism::ParseResult<'_>,
    hir: &mut Hir,
    const_name: &str,
    value: &Node<'_>,
) -> Option<PResult<NodeId>> {
    let call = value.as_call_node()?;
    let recv = call.receiver()?;
    let recv = recv.as_constant_read_node()?;
    if recv.name().as_slice() != b"Data" || call.name().as_slice() != b"define" {
        return None;
    }
    Some(lower_value_def(ValueKind::Data, result, hir, const_name, &call))
}

/// Parse the member symbols (and, for `Struct`, an optional literal
/// `keyword_init:`). Shared by both value-class kinds.
fn parse_members(kind: ValueKind, call: &ruby_prism::CallNode<'_>) -> PResult<(Vec<String>, bool)> {
    let ctor = match kind {
        ValueKind::Struct => "Struct.new",
        ValueKind::Data => "Data.define",
    };
    let mut members: Vec<String> = Vec::new();
    let mut keyword_init = false;
    if let Some(arguments) = call.arguments() {
        for arg in arguments.arguments().iter() {
            if let Some(sym) = arg.as_symbol_node() {
                members.push(String::from_utf8_lossy(sym.unescaped()).into_owned());
                continue;
            }
            if let Some(kw) = arg.as_keyword_hash_node() {
                if kind == ValueKind::Data {
                    return Err("Data.define takes only member symbols (no keyword options)".to_string());
                }
                for pair in kw.elements().iter() {
                    let assoc = pair
                        .as_assoc_node()
                        .ok_or("Struct.new keyword arguments must be literal (spike scope)")?;
                    let key = assoc
                        .key()
                        .as_symbol_node()
                        .map(|s| String::from_utf8_lossy(s.unescaped()).into_owned())
                        .ok_or("Struct.new keyword arguments must be literal symbols")?;
                    if key != "keyword_init" {
                        return Err(format!(
                            "Struct.new option `{key}:` isn't supported (spike scope; only keyword_init:)"
                        ));
                    }
                    let value = assoc.value();
                    keyword_init = if value.as_true_node().is_some() {
                        true
                    } else if value.as_false_node().is_some() {
                        false
                    } else {
                        return Err(
                            "Struct.new's keyword_init: must be a literal true/false (spike scope)"
                                .to_string(),
                        );
                    };
                }
                continue;
            }
            return Err(format!(
                "{ctor} members must be literal symbols (spike scope; the `{ctor}(\"Name\", ...)` string form isn't supported)"
            ));
        }
    }
    if members.is_empty() {
        return Err(format!("{ctor} needs at least one member symbol (spike scope)"));
    }
    Ok((members, keyword_init))
}

fn lower_value_def(
    kind: ValueKind,
    result: &ruby_prism::ParseResult<'_>,
    hir: &mut Hir,
    const_name: &str,
    call: &ruby_prism::CallNode<'_>,
) -> PResult<NodeId> {
    let (members, keyword_init) = parse_members(kind, call)?;

    // Lower the user block's `def`s first (with the ORIGINAL parse result);
    // whether one of them is a custom `initialize` decides the class shape.
    let extra = lower_block_methods(result, hir, call)?;
    let has_custom_init = extra
        .iter()
        .any(|&id| matches!(&hir[id], HirNode::DefMethod { name, .. } if name.as_str() == "initialize"));

    if !has_custom_init {
        // Single-level: the template's own member-setter `initialize` is the
        // only one (`super` is never needed), so the user-facing class IS the
        // synthesized class. No spurious ancestor -- the common case.
        let src = value_template(kind, const_name, const_name, &members, keyword_init);
        let class_id = lower_synth_class(hir, &src, const_name)?;
        append_methods(hir, class_id, extra)?;
        return Ok(class_id);
    }

    // Two-level: a custom `initialize` in the block must be able to
    // `super` into the member-setter. So the member-setter (+ accessors,
    // deconstruct, ==, inspect...) lives on a hidden BASE class, and the
    // user-facing LEAF inherits it -- the user `initialize` overrides on the
    // leaf and its `super` resolves the base's member-setter through the
    // ordinary ancestor walk. Real Ruby puts the member-setter on
    // `Struct`/`Data` itself; the base is our monomorphized stand-in for it.
    let base_name = format!("{const_name}__ValueBase");
    let base_src = value_template(kind, &base_name, const_name, &members, keyword_init);
    let base_id = lower_synth_class(hir, &base_src, const_name)?;
    hir.synth_classes.push(base_id);

    let leaf_src = format!("class {const_name} < {base_name}\nend\n");
    let leaf_id = lower_synth_class(hir, &leaf_src, const_name)?;
    append_methods(hir, leaf_id, extra)?;
    Ok(leaf_id)
}

/// The per-kind template dispatcher (`class_name` is declared; `display` is
/// baked into `inspect`/`==`/`with` -- they differ for a two-level base).
fn value_template(
    kind: ValueKind,
    class_name: &str,
    display: &str,
    members: &[String],
    keyword_init: bool,
) -> String {
    match kind {
        ValueKind::Struct => struct_template(class_name, display, members, keyword_init),
        ValueKind::Data => data_template(class_name, display, members),
    }
}

/// Parse one synthesized single-class Ruby source and lower its class
/// statement into `hir`.
fn lower_synth_class(hir: &mut Hir, src: &str, ctx: &str) -> PResult<NodeId> {
    let parsed = ruby_prism::parse(src.as_bytes());
    if parsed.errors().next().is_some() {
        return Err(format!(
            "internal error: the synthesized value-class template for `{ctx}` failed to parse"
        ));
    }
    let program = parsed
        .node()
        .as_program_node()
        .ok_or("internal error: value-class template has no program root")?;
    let class_stmt = program
        .statements()
        .body()
        .iter()
        .next()
        .ok_or("internal error: value-class template is empty")?;
    lower_node(&parsed, hir, &class_stmt)
}

/// Lower the `do ... end` block's method defs (the block-with-methods form),
/// with the ORIGINAL parse result. Empty when there's no block.
fn lower_block_methods(
    result: &ruby_prism::ParseResult<'_>,
    hir: &mut Hir,
    call: &ruby_prism::CallNode<'_>,
) -> PResult<Vec<NodeId>> {
    let mut extra = Vec::new();
    if let Some(block) = call.block() {
        let block = block
            .as_block_node()
            .ok_or("a value class's block can't be a block-argument forward (spike scope)")?;
        if let Some(body) = block.body() {
            let statements = body
                .as_statements_node()
                .ok_or("expected statements in the value class's block")?;
            let mut visibility = Visibility::Public;
            let mut module_function = false;
            for stmt in statements.body().iter() {
                lower_class_body_statement(
                    result,
                    hir,
                    &stmt,
                    &mut visibility,
                    &mut module_function,
                    &mut extra,
                )?;
            }
        }
    }
    Ok(extra)
}

/// Append the user block's method defs to a synthesized class body
/// (later-def-wins lets a user `def` override a template method).
fn append_methods(hir: &mut Hir, class_id: NodeId, extra: Vec<NodeId>) -> PResult<()> {
    if extra.is_empty() {
        return Ok(());
    }
    let HirNode::ClassDef { body, .. } = &mut hir[class_id] else {
        return Err("internal error: value-class template didn't lower to a class".to_string());
    };
    body.extend(extra);
    Ok(())
}

/// The per-struct method template. Everything specializes at generation
/// time (members are compile-time-known), the AOT-natural monomorphization
/// -- same philosophy as `mro::materialize`. `name` is the class being
/// declared; `display` is the user-facing name baked into `inspect`/`==`
/// (they differ only for the hidden base of a custom-`initialize` split).
fn struct_template(name: &str, display: &str, members: &[String], keyword_init: bool) -> String {
    let n = members.len();
    let accessors = members
        .iter()
        .map(|m| format!(":{m}"))
        .collect::<Vec<_>>()
        .join(", ");
    // keyword_init binds through an options hash (`HirNode::New` has no
    // kwargs channel -- see parse's `.new` lowering note); positional
    // members nil-fill.
    let init_params = if keyword_init {
        "opts = {}".to_string()
    } else {
        members
            .iter()
            .map(|m| format!("{m} = nil"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let init_body = if keyword_init {
        members
            .iter()
            .map(|m| format!("    @{m} = opts[:{m}]"))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        members
            .iter()
            .map(|m| format!("    @{m} = {m}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let member_list = members
        .iter()
        .map(|m| format!(":{m}"))
        .collect::<Vec<_>>()
        .join(", ");
    let value_list = members.join(", ");
    let hash_pairs = members
        .iter()
        .map(|m| format!("{m}: {m}"))
        .collect::<Vec<_>>()
        .join(", ");
    let each_yields = members
        .iter()
        .map(|m| format!("    yield {m}"))
        .collect::<Vec<_>>()
        .join("\n");
    let each_pair_yields = members
        .iter()
        .map(|m| format!("    yield :{m}, {m}"))
        .collect::<Vec<_>>()
        .join("\n");
    // `s[0]`, `s[-N]`, `s[:x]`, `s["x"]` all address member 0, etc.
    let index_when_arms = |write: bool| -> String {
        members
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let neg = i as i64 - n as i64;
                let body = if write {
                    format!("@{m} = v")
                } else {
                    m.clone()
                };
                format!("    when {i}, {neg}, :{m}, \"{m}\" then {body}")
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let read_arms = index_when_arms(false);
    let write_arms = index_when_arms(true);
    let inspect_parts = members
        .iter()
        .map(|m| format!("{m}=#{{{m}.inspect}}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r##"class {name} < Struct
  def initialize({init_params})
{init_body}
  end
  def members
    [{member_list}]
  end
  def to_a
    [{value_list}]
  end
  def deconstruct
    to_a
  end
  def to_h
    {{ {hash_pairs} }}
  end
  def deconstruct_keys(keys)
    to_h
  end
  def ==(other)
    other.is_a?({display}) && other.to_a == to_a
  end
  def each
    return to_enum(:each) unless block_given?
{each_yields}
    self
  end
  def each_pair
    return to_enum(:each_pair) unless block_given?
{each_pair_yields}
    self
  end
  def length
    {n}
  end
  def size
    {n}
  end
  def [](k)
    case k
{read_arms}
    end
  end
  def []=(k, v)
    case k
{write_arms}
    end
  end
  def inspect
    "#<struct {display} {inspect_parts}>"
  end
  def to_s
    inspect
  end
  attr_accessor {accessors}
end
"##
    )
}

/// The per-Data method template -- the immutable sibling of
/// `struct_template`. `Data.define`'d classes construct by keyword only
/// (`Point.new(x: 1, y: 2)`; a missing member is CRuby's `missing keyword`),
/// expose readers (no writers), and answer `deconstruct`/`deconstruct_keys`/
/// `to_h`/`members`/`with`/`==`/`hash`/`inspect`. Not `Enumerable` (no
/// `each`). Positional construction (`Point.new(1, 2)`) is a documented
/// follow-on -- keyword construction is what real code and every test uses.
fn data_template(name: &str, display: &str, members: &[String]) -> String {
    let n = members.len();
    // Positional construction (`Point.new(1, 2)`) binds members in order;
    // keyword construction (`Point.new(x: 1, y: 2)`) binds by name. An empty
    // call with members takes the keyword path (so it reports missing keywords).
    let pos_assigns = members
        .iter()
        .enumerate()
        .map(|(i, m)| format!("      @{m} = args[{i}]"))
        .collect::<Vec<_>>()
        .join("\n");
    let kw_fetches = members
        .iter()
        .map(|m| format!("      @{m} = kwargs.fetch(:{m}) {{ __missing << :{m}; nil }}"))
        .collect::<Vec<_>>()
        .join("\n");
    let accessors = members
        .iter()
        .map(|m| format!(":{m}"))
        .collect::<Vec<_>>()
        .join(", ");
    let member_list = accessors.clone();
    let ivar_list = members
        .iter()
        .map(|m| format!("@{m}"))
        .collect::<Vec<_>>()
        .join(", ");
    let hash_pairs = members
        .iter()
        .map(|m| format!("{m}: @{m}"))
        .collect::<Vec<_>>()
        .join(", ");
    // `with` maps each member from the changes hash, defaulting to the
    // current value -- monomorphized to explicit keywords so it needs no
    // `**h`-at-`.new` (which lands in F6).
    let with_args = members
        .iter()
        .map(|m| format!("{m}: changes.fetch(:{m}, @{m})"))
        .collect::<Vec<_>>()
        .join(", ");
    let inspect_parts = members
        .iter()
        .map(|m| format!("{m}=#{{@{m}.inspect}}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r##"class {name} < Data
  def initialize(*args, **kwargs)
    if !kwargs.empty? || (args.empty? && {n} > 0)
      unless args.empty?
        raise ArgumentError, "wrong number of arguments (given #{{args.size}}, expected 0)"
      end
      __missing = []
{kw_fetches}
      unless __missing.empty?
        raise ArgumentError, (__missing.size == 1 ? "missing keyword: #{{__missing.first.inspect}}" : "missing keywords: #{{__missing.map(&:inspect).join(', ')}}")
      end
      __extra = kwargs.keys - [{member_list}]
      unless __extra.empty?
        raise ArgumentError, (__extra.size == 1 ? "unknown keyword: #{{__extra.first.inspect}}" : "unknown keywords: #{{__extra.map(&:inspect).join(', ')}}")
      end
    else
      unless args.size == {n}
        raise ArgumentError, "wrong number of arguments (given #{{args.size}}, expected {n})"
      end
{pos_assigns}
    end
    freeze
  end
  def members
    [{member_list}]
  end
  def to_h
    {{ {hash_pairs} }}
  end
  def deconstruct
    [{ivar_list}]
  end
  def deconstruct_keys(keys)
    to_h
  end
  def with(changes = {{}})
    __extra = changes.keys - [{member_list}]
    unless __extra.empty?
      raise ArgumentError, (__extra.size == 1 ? "unknown keyword: #{{__extra.first.inspect}}" : "unknown keywords: #{{__extra.map(&:inspect).join(', ')}}")
    end
    {display}.new({with_args})
  end
  def ==(other)
    other.is_a?({display}) && other.to_h == to_h
  end
  def eql?(other)
    self == other
  end
  def hash
    to_h.hash
  end
  def inspect
    "#<data {display} {inspect_parts}>"
  end
  def to_s
    inspect
  end
  attr_reader {accessors}
end
"##
    )
}
