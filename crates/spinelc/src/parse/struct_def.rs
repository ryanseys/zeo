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
    Some(lower_struct_def(result, hir, const_name, &call))
}

fn lower_struct_def(
    result: &ruby_prism::ParseResult<'_>,
    hir: &mut Hir,
    const_name: &str,
    call: &ruby_prism::CallNode<'_>,
) -> PResult<NodeId> {
    let mut members: Vec<String> = Vec::new();
    let mut keyword_init = false;
    if let Some(arguments) = call.arguments() {
        for arg in arguments.arguments().iter() {
            if let Some(sym) = arg.as_symbol_node() {
                members.push(String::from_utf8_lossy(sym.unescaped()).into_owned());
                continue;
            }
            if let Some(kw) = arg.as_keyword_hash_node() {
                for pair in kw.elements().iter() {
                    let assoc = pair.as_assoc_node().ok_or(
                        "Struct.new keyword arguments must be literal (spike scope)",
                    )?;
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
            return Err(
                "Struct.new members must be literal symbols (spike scope; the `Struct.new(\"Name\", ...)` string form isn't supported)"
                    .to_string(),
            );
        }
    }
    if members.is_empty() {
        return Err("Struct.new needs at least one member symbol (spike scope)".to_string());
    }

    // The synthesized class -- ordinary Ruby source, monomorphized for
    // this exact member list, fed back through the ordinary parser.
    let src = struct_template(const_name, &members, keyword_init);
    let parsed = ruby_prism::parse(src.as_bytes());
    if parsed.errors().next().is_some() {
        return Err(format!(
            "internal error: the synthesized Struct template for `{const_name}` failed to parse"
        ));
    }
    let program = parsed
        .node()
        .as_program_node()
        .ok_or("internal error: Struct template has no program root")?;
    let statements = program.statements();
    let class_stmt = statements
        .body()
        .iter()
        .next()
        .ok_or("internal error: Struct template is empty")?;
    let class_id = lower_node(&parsed, hir, &class_stmt)?;

    // The block-with-methods form: `Point = Struct.new(:x, :y) do ... end`
    // -- its defs lower with the ORIGINAL parse result and append after
    // the template's (later-def-wins lets them override).
    if let Some(block) = call.block() {
        let block = block
            .as_block_node()
            .ok_or("Struct.new's block can't be a block-argument forward (spike scope)")?;
        let mut extra = Vec::new();
        if let Some(body) = block.body() {
            let statements = body
                .as_statements_node()
                .ok_or("expected statements in Struct.new's block")?;
            let mut visibility = Visibility::Public;
            for stmt in statements.body().iter() {
                lower_class_body_statement(result, hir, &stmt, &mut visibility, &mut extra)?;
            }
        }
        let HirNode::ClassDef { body, .. } = &mut hir[class_id] else {
            return Err("internal error: Struct template didn't lower to a class".to_string());
        };
        body.extend(extra);
    }

    Ok(class_id)
}

/// The per-struct method template. Everything specializes at generation
/// time (members are compile-time-known), the AOT-natural
/// monomorphization -- same philosophy as `mro::materialize`.
fn struct_template(name: &str, members: &[String], keyword_init: bool) -> String {
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
    other.is_a?({name}) && other.to_a == to_a
  end
  def each
{each_yields}
    self
  end
  def each_pair
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
    "#<struct {name} {inspect_parts}>"
  end
  def to_s
    inspect
  end
  attr_accessor {accessors}
end
"##
    )
}
