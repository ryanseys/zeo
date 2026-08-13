//! The FFI directive family (the real `ffi` gem's idioms): `extend
//! FFI::Library` recognition, `ffi_lib`/`typedef`/`enum`/`callback`/
//! `attach_function` lowering, the `class < FFI::Struct` `layout` ->
//! accessor-method synthesis, and the C type-name mapping shared by both.
//! Split out of `parse/mod.rs`.

#![allow(
    clippy::wildcard_enum_match_arm,
    reason = "not yet swept for wildcard arms -- see the lint's note in lib.rs"
)]

use super::PResult;
use super::literals::assemble_i64;
use crate::hir::{Hir, HirNode, NodeId, Params, Visibility};
use ruby_prism::{Node, ParseResult};

/// One FFI declaration harvested BEFORE lowering, for the program-wide table
/// only -- see `Loader::lower_file_statements`, which runs this over a file's
/// own bodies ahead of the class-body requires that pre-lower under them.
///
/// Deliberately narrow: only the shapes that need no alias table at all, so
/// the answer cannot differ from what real lowering will register a moment
/// later. Anything else is left alone; the real pass is still the one that
/// decides.
///
/// fast_excel is the case this exists for. It declares `enum :error, [...]` at
/// binding.rb:318 and requires `binding/chart.rb` at 714 -- the source order is
/// already right, but ALL nested requires pre-lower before ANY of the
/// requiring file's statements, so the sub-file saw no vocabulary at all.
pub(crate) fn prescan_declaration(hir: &mut Hir, node: &Node<'_>) {
    let Some(call) = node.as_call_node() else {
        return;
    };
    if call.receiver().is_some() {
        return;
    }
    let args: Vec<Node<'_>> = call
        .arguments()
        .map(|a| a.arguments().iter().collect())
        .unwrap_or_default();
    match call.name().as_slice() {
        // `enum :tag, [:a, 0, :b, 1]` with every member written out. A member
        // list this pass cannot read is left to the real lowering, which has
        // the deferred tier for it.
        b"enum" => {
            let [tag, members] = args.as_slice() else {
                return;
            };
            let (Ok(tag), Ok(members)) = (
                ffi_symbol_str(tag),
                parse_enum_members(std::slice::from_ref(members), hir, &[], &[]),
            ) else {
                return;
            };
            hir.declare_ffi_type(&tag, &crate::hir::FfiType::Enum(members));
        }
        // `typedef :ulong, :XID` -- both sides literal, the source a keyword
        // the shared table already knows.
        b"typedef" => {
            let [src, alias] = args.as_slice() else {
                return;
            };
            let (Ok(src), Ok(alias)) = (ffi_symbol_str(src), ffi_symbol_str(alias)) else {
                return;
            };
            let Ok(ty) = ffi_type_of(&src, &Default::default()) else {
                return;
            };
            hir.declare_ffi_type(&alias, &ty);
        }
        _ => {}
    }
}

/// A `def self.included(base)` hook whose body runs `base.class_eval` over a
/// block containing a `layout` -- the struct-side twin of
/// [`ffi_extender_hook`]. Returns the block body's SOURCE, which is what a
/// later `include <this module>` replays.
///
/// Source text rather than nodes: a prism `Node` is neither `Clone` nor
/// storable past its `ParseResult`, and the replay happens in another file
/// entirely. gssapi is the case -- `GssBufferDescLayout` carries the layout AND
/// the two readers for every buffer struct in the gem.
pub(crate) fn ffi_layout_hook(result: &ParseResult, node: &Node<'_>) -> Option<String> {
    let def = node.as_def_node()?;
    def.receiver()?.as_self_node()?;
    if def.name().as_slice() != b"included" {
        return None;
    }
    let requireds: Vec<_> = def.parameters()?.requireds().iter().collect();
    let [host] = requireds.as_slice() else {
        return None;
    };
    let host = host.as_required_parameter_node()?.name();
    for stmt in def.body()?.as_statements_node()?.body().iter() {
        let Some(call) = stmt.as_call_node() else {
            continue;
        };
        if call.name().as_slice() != b"class_eval"
            || !call
                .receiver()
                .and_then(|r| r.as_local_variable_read_node())
                .is_some_and(|l| l.name().as_slice() == host.as_slice())
        {
            continue;
        }
        let body = call.block()?.as_block_node()?.body()?;
        let stmts = body.as_statements_node()?;
        // A block with no `layout` is somebody else's DSL, not this one.
        if !stmts.body().iter().any(|s| {
            s.as_call_node()
                .is_some_and(|c| c.receiver().is_none() && c.name().as_slice() == b"layout")
        }) {
            continue;
        }
        let span = body.location();
        let src = result.source();
        return String::from_utf8(src[span.start_offset()..span.end_offset()].to_vec()).ok();
    }
    None
}

/// [`prescan_declaration`] for a `layout` inside an `FFI::Struct` body.
///
/// A field this pass cannot resolve records the CLASS only -- the
/// by-reference fact every signature position needs, and the one a struct has
/// whether or not its fields are known yet. The real lowering runs with a
/// superset of this table and is still the one that decides; a name cannot
/// come to mean two things, because `declare_ffi_type` poisons a conflicting
/// redeclaration and a recomputed layout simply overwrites.
pub(crate) fn prescan_layout(hir: &mut Hir, node: &Node<'_>, class_path: &str, union: bool) {
    let leaf = class_path.rsplit("::").next().unwrap_or(class_path);
    hir.mark_ffi_struct_class(leaf);
    let aliases = hir.inherited_ffi_types();
    let Ok(Some(fields)) = as_ffi_layout(node, &aliases, hir, &[], &[]) else {
        return;
    };
    let Ok(layout) = ffi_struct_layout(class_path, &fields, union) else {
        return;
    };
    hir.ffi_struct_layouts.insert(leaf.to_string(), layout);
}

/// `extend FFI::Library` -- the marker that turns a module into an FFI library
/// (the real `ffi` gem's idiom). Recognized syntactically so the `FFI::Library`
/// constant never has to resolve at runtime.
pub(crate) fn is_extend_ffi_library(node: &Node<'_>) -> bool {
    let Some(call) = node.as_call_node() else {
        return false;
    };
    if call.receiver().is_some() || call.name().as_slice() != b"extend" {
        return false;
    }
    let Some(args) = call.arguments() else {
        return false;
    };
    let mut it = args.arguments().iter();
    match (it.next(), it.next()) {
        (Some(arg), None) => const_path_string(&arg).as_deref() == Some("FFI::Library"),
        _ => false,
    }
}

/// `extend FFI::DataConverter` -- the class converts to/from a native FFI
/// type it names with `native_type`. Recognized so the CLASS NAME itself
/// works in later type positions (google-protobuf's `Internal::Arena` is
/// `native_type ::FFI::Type::POINTER`, then appears in `attach_function`
/// argument lists 104 corpus rows deep).
pub(crate) fn is_extend_ffi_data_converter(node: &Node<'_>) -> bool {
    let Some(call) = node.as_call_node() else {
        return false;
    };
    if call.receiver().is_some() || call.name().as_slice() != b"extend" {
        return false;
    }
    let Some(args) = call.arguments() else {
        return false;
    };
    let mut it = args.arguments().iter();
    match (it.next(), it.next()) {
        (Some(arg), None) => const_path_string(&arg).as_deref() == Some("FFI::DataConverter"),
        _ => false,
    }
}

/// `FFI.typedef :existing, :alias` -- the GLOBAL type registry the gem keeps
/// on the FFI module itself, consulted by every library module and struct
/// layout program-wide (puppet declares the Win32 vocabulary this way in one
/// file and spends it across sibling files). Returns the resolved type and
/// the new alias name; `None` (fall through to ordinary call lowering) for
/// any other statement, or for a source type zeo cannot resolve -- a
/// platform-varying typedef must stay an honest rejection at its USE site,
/// not a wrong width registered here.
pub(crate) fn as_global_ffi_typedef(
    node: &Node<'_>,
    aliases: &crate::compiler::FMap<String, crate::hir::FfiType>,
    cref: Option<&str>,
) -> Option<(crate::hir::FfiType, String)> {
    let call = node.as_call_node()?;
    if call.name().as_slice() != b"typedef" {
        return None;
    }
    match call.receiver() {
        Some(recv) => {
            if const_path_string(&recv).as_deref() != Some("FFI") {
                return None;
            }
        }
        // A REOPENED `module ::FFI` body: self is the FFI module, so a bare
        // `typedef :ulong, :XID` is the same global declaration `FFI.typedef`
        // makes. x11 fills the whole X vocabulary that way, in one file, and
        // spends it across every other one.
        None => {
            let cref = cref?.trim_start_matches("::");
            if cref != "FFI" && !cref.ends_with("::FFI") {
                return None;
            }
        }
    }
    let args: Vec<Node<'_>> = call.arguments()?.arguments().iter().collect();
    let [existing, alias] = args.as_slice() else {
        return None;
    };
    let existing = ffi_type_node(existing, aliases, TypePos::Signature).ok()?;
    let alias = ffi_symbol_str(alias).ok()?;
    Some((existing, alias))
}

/// A `def self.extended(host)` hook whose body runs `host.extend FFI::Library`
/// -- the indirection chef's Win32 API modules share one FFI setup through.
/// The hook makes every module that later `extend`s ITS module an FFI library,
/// so the recognizer returns the hook's flat `host.typedef :src, :alias`
/// stream for the extend site to replay into its own alias table. Statements
/// under a conditional inside the hook are NOT replayed (chef gates two
/// typedefs on an ENV probe); a type they would have declared stays
/// undeclared, and a later use of it is an honest rejection.
pub(crate) fn ffi_extender_hook(node: &Node<'_>) -> Option<Vec<(String, String)>> {
    let def = node.as_def_node()?;
    def.receiver()?.as_self_node()?;
    if def.name().as_slice() != b"extended" {
        return None;
    }
    let requireds: Vec<_> = def.parameters()?.requireds().iter().collect();
    let [host] = requireds.as_slice() else {
        return None;
    };
    let host = host.as_required_parameter_node()?.name();
    let stmts = def.body()?.as_statements_node()?;
    let mut extends_library = false;
    let mut pairs = Vec::new();
    for stmt in stmts.body().iter() {
        let Some(call) = stmt.as_call_node() else {
            continue;
        };
        let Some(recv) = call.receiver() else {
            continue;
        };
        let is_host = recv
            .as_local_variable_read_node()
            .is_some_and(|l| l.name().as_slice() == host.as_slice());
        if !is_host {
            continue;
        }
        let args: Vec<Node<'_>> = call
            .arguments()
            .map(|a| a.arguments().iter().collect())
            .unwrap_or_default();
        match call.name().as_slice() {
            b"extend" => {
                extends_library |= args
                    .iter()
                    .any(|a| const_path_string(a).as_deref() == Some("FFI::Library"));
            }
            b"typedef" => {
                if let [src, alias] = args.as_slice()
                    && let (Ok(src), Ok(alias)) = (ffi_symbol_str(src), ffi_symbol_str(alias))
                {
                    pairs.push((src, alias));
                }
            }
            _ => {}
        }
    }
    extends_library.then_some(pairs)
}

/// The constant path a bare `extend SomeConst` statement names -- how a class
/// body asks whether the target is a recorded [`ffi_extender_hook`] module.
pub(crate) fn extend_target_path(node: &Node<'_>) -> Option<String> {
    let call = node.as_call_node()?;
    if call.receiver().is_some() || call.name().as_slice() != b"extend" {
        return None;
    }
    let args: Vec<Node<'_>> = call.arguments()?.arguments().iter().collect();
    let [target] = args.as_slice() else {
        return None;
    };
    const_path_string(target)
}

/// The native type a `native_type <T>` statement declares, spelled as a
/// symbol (`native_type :pointer`) or an `FFI::Type::X` constant.
pub(crate) fn native_type_of(node: &Node<'_>) -> Option<crate::hir::FfiType> {
    let call = node.as_call_node()?;
    if call.receiver().is_some() || call.name().as_slice() != b"native_type" {
        return None;
    }
    let mut it = call.arguments()?.arguments().iter();
    let (arg, None) = (it.next()?, it.next()) else {
        return None;
    };
    if let Some(sym) = arg.as_symbol_node() {
        let empty = crate::compiler::FMap::default();
        return ffi_type_of(&String::from_utf8_lossy(sym.unescaped()), &empty).ok();
    }
    let path = const_path_string(&arg)?;
    ffi_type_constant(path.trim_start_matches("::").strip_prefix("FFI::Type::")?)
}

/// The `FFI::Type::X` constants -- `zeo_abi::ffi::CScalar`'s table, which the
/// runtime's `FFI::Type` objects also answer to.
fn ffi_type_constant(leaf: &str) -> Option<crate::hir::FfiType> {
    zeo_abi::ffi::CScalar::from_type_constant(leaf).map(Into::into)
}

/// `FFI::Type::LONG_LONG` as a type, from a full constant path spelled with
/// or without the leading `::` (audio's `CFIndex = FFI::Type::LONG_LONG`).
pub(crate) fn ffi_type_constant_of(path: &str) -> Option<crate::hir::FfiType> {
    ffi_type_constant(path.trim_start_matches("::").strip_prefix("FFI::Type::")?)
}

/// The width and signedness a POSIX integer typedef actually has, read off
/// the `libc` crate THIS compiler was built against.
///
/// These are the typedefs whose width genuinely differs between targets
/// (`sa_family_t` is one byte on macOS and two on glibc; `dev_t` is signed
/// 32-bit on one and unsigned 64-bit on the other) -- which is why a
/// signature spells the target's own `libc::<name>` and lets rustc decide.
/// A struct FIELD cannot wait that long: its width fixes every following
/// field's offset at lowering time. Answering from the compiler's own libc is
/// exact for the one target zeo emits for -- the same assumption the baked
/// `RUBY_PLATFORM` and `RbConfig::CONFIG` already make, and there is no
/// cross-compilation mode for them to disagree with.
pub(crate) fn platform_scalar_of(name: &str) -> Option<zeo_abi::ffi::CScalar> {
    use zeo_abi::ffi::CScalar;
    fn scalar(size: usize, signed: bool) -> Option<CScalar> {
        Some(match (size, signed) {
            (1, true) => CScalar::I8,
            (1, false) => CScalar::U8,
            (2, true) => CScalar::I16,
            (2, false) => CScalar::U16,
            (4, true) => CScalar::I32,
            (4, false) => CScalar::U32,
            (8, true) => CScalar::I64,
            (8, false) => CScalar::U64,
            _ => return None,
        })
    }
    macro_rules! host_widths {
        ($($spelling:literal => $ty:ty),* $(,)?) => {
            match name {
                // `MIN != 0` reads signedness without tripping rustc's
                // useless-comparison lint on the unsigned rows.
                $($spelling => scalar(size_of::<$ty>(), <$ty>::MIN != 0),)*
                _ => None,
            }
        };
    }
    host_widths! {
        "mode_t" => libc::mode_t,
        "dev_t" => libc::dev_t,
        "nlink_t" => libc::nlink_t,
        "sa_family_t" => libc::sa_family_t,
        "blksize_t" => libc::blksize_t,
        "suseconds_t" => libc::suseconds_t,
        "clock_t" => libc::clock_t,
    }
}

/// The elements of the body-local ARRAY a lone `*splat` argument names, read
/// back from the class-body statements written before `directive`.
///
/// Gems build a declaration list conditionally and splat it in one go --
/// sys-uname's `utsname` layout gains a `domainname` field on linux and an
/// `__id_number` on hpux, then calls `layout(*members)`. The array is a
/// compile-time value: literal assignments plus `push`/`<<`/`concat` calls,
/// under guards this stage already folds. Anything else that so much as
/// MENTIONS the local declines the whole read, so a list zeo cannot follow
/// stays the loud rejection it is today instead of becoming a short struct.
pub(crate) fn splat_local_elements<'a>(
    args: &[Node<'a>],
    body: &[Node<'a>],
    directive: &Node<'_>,
) -> Option<Vec<Node<'a>>> {
    let [only] = args else { return None };
    let expr = only.as_splat_node()?.expression()?;
    if let Some(elems) = splatted_literal_elements(&expr) {
        return Some(elems);
    }
    let name = local_read_name(&expr)?;
    local_array_elements(&name, body, directive.location().start_offset())
}

/// The elements a splatted LITERAL contributes, needing no replay at all.
///
/// `layout(*[ :cbSize, :uint, ... ])` is the same flat pair list written with
/// a splat in front of it -- three Win32 gems spell `NONCLIENTMETRICS` that
/// way. `layout(*{ acceleration_mode: :int32, ... }.to_a.flatten)` is the
/// documented hash spelling put through the flattening the splat then undoes
/// (voicevox); `Hash#to_a.flatten` on a hash of symbol keys IS the pair list,
/// so reading it as one is the value the program computes, not a guess.
fn splatted_literal_elements<'a>(node: &Node<'a>) -> Option<Vec<Node<'a>>> {
    if let Some(array) = node.as_array_node() {
        let elements: Vec<Node<'a>> = array.elements().iter().collect();
        return elements
            .iter()
            .all(|e| e.as_splat_node().is_none())
            .then_some(elements);
    }
    let flatten = node.as_call_node()?;
    if flatten.name().as_slice() != b"flatten" || flatten.arguments().is_some() {
        return None;
    }
    let to_a = flatten.receiver()?;
    let to_a = to_a.as_call_node()?;
    if to_a.name().as_slice() != b"to_a" || to_a.arguments().is_some() {
        return None;
    }
    let mut out = Vec::new();
    for (k, v) in hash_pairs(&to_a.receiver()?)? {
        out.push(k);
        out.push(v);
    }
    Some(out)
}

/// [`splat_local_elements`] for a list named WITHOUT a splat -- `enum :colour,
/// members` hands the array over directly.
pub(crate) fn local_array_elements<'a>(
    name: &str,
    body: &[Node<'a>],
    before: usize,
) -> Option<Vec<Node<'a>>> {
    let mut acc: Option<Vec<Node<'a>>> = None;
    replay_local_array(name, body.iter(), before, &mut acc).then_some(acc)?
}

/// `%w(160k 320k 96k).map(&:to_sym)` -- an enum member list written as a word
/// array. `String#to_sym` is what `ffi`'s enum does to a string member anyway,
/// so the mapped list IS the literal one; spotify spells four of its enums
/// this way. The elements come back as the STRING nodes, which every member
/// reader already accepts.
fn word_list_to_syms<'a>(node: &Node<'a>) -> Option<Vec<Node<'a>>> {
    let call = node.as_call_node()?;
    if call.name().as_slice() != b"map" || call.arguments().is_some() {
        return None;
    }
    let block = call.block()?.as_block_argument_node()?.expression()?;
    if block.as_symbol_node()?.unescaped() != b"to_sym" {
        return None;
    }
    let elements: Vec<Node<'a>> = call
        .receiver()?
        .as_array_node()?
        .elements()
        .iter()
        .collect();
    elements
        .iter()
        .all(|e| e.as_string_node().is_some() || e.as_symbol_node().is_some())
        .then_some(elements)
}

/// The C symbol name an FFI declaration position spells, folding the
/// class-body arithmetic gems build it out of.
///
/// A library whose windows entry points carry an `A` suffix writes one
/// `str_suffix = FFI::Platform.windows? ? 'A' : ''` at the top of the module
/// and then `'SCardListReaderGroups' + str_suffix` at every declaration --
/// smartcard does it nine times. The suffix is a compile-time fact (the same
/// platform question the guards fold), so the symbol is too. Anything that
/// does not fold falls back to the literal-only reading, whose rejection
/// names the real rule.
fn ffi_c_name(node: &Node<'_>, class_body: &[Node<'_>]) -> PResult<String> {
    match static_ffi_string(node, class_body, 0) {
        Some(s) => Ok(s),
        None => ffi_symbol_str(node),
    }
}

/// A compile-time STRING in an FFI declaration: a literal, a `+` of two of
/// them, a body-local holding one, or a guard this stage decides choosing
/// between two.
fn static_ffi_string(node: &Node<'_>, class_body: &[Node<'_>], depth: u32) -> Option<String> {
    if depth > 8 {
        return None;
    }
    if let Some(s) = node.as_string_node() {
        return String::from_utf8(s.unescaped().to_vec()).ok();
    }
    if let Some(sym) = node.as_symbol_node() {
        return String::from_utf8(sym.unescaped().to_vec()).ok();
    }
    // `cond ? 'A' : ''` and its statement form both parse as an `if`.
    if let Some(if_node) = node.as_if_node() {
        let taken = match super::defs::static_guard(&if_node.predicate())? {
            true => if_node.statements().map(|s| s.as_node()),
            false => if_node.subsequent(),
        };
        return static_ffi_string(&sole_statement(taken?)?, class_body, depth + 1);
    }
    if let Some(name) = local_read_name(node) {
        let value = body_local_value(&name, class_body, node.location().start_offset())?;
        return static_ffi_string(&value, class_body, depth + 1);
    }
    let call = node.as_call_node()?;
    if call.name().as_slice() != b"+" {
        return None;
    }
    let args: Vec<Node<'_>> = call.arguments()?.arguments().iter().collect();
    let [rhs] = args.as_slice() else {
        return None;
    };
    let lhs = static_ffi_string(&call.receiver()?, class_body, depth + 1)?;
    Some(lhs + &static_ffi_string(rhs, class_body, depth + 1)?)
}

/// The single expression a folded branch holds -- an `else` clause or a
/// statement list of exactly one.
fn sole_statement<'a>(node: Node<'a>) -> Option<Node<'a>> {
    let stmts = match node.as_else_node() {
        Some(e) => e.statements()?,
        None => node.as_statements_node()?,
    };
    let body: Vec<Node<'a>> = stmts.body().iter().collect();
    match <[Node<'a>; 1]>::try_from(body) {
        Ok([only]) => Some(only),
        Err(_) => None,
    }
}

/// The expression a body-local was last assigned before `before`, or `None`
/// when the class body touches the name in a way this replay cannot follow --
/// the same poison rule [`splat_local_elements`] uses.
fn body_local_value<'a>(name: &str, body: &[Node<'a>], before: usize) -> Option<Node<'a>> {
    let mut found: Option<Node<'a>> = None;
    for stmt in body {
        // Compared on the END offset: `before` points at the READ, which sits
        // inside the directive statement itself. Comparing starts would let
        // that statement poison the very name it is reading.
        if stmt.location().end_offset() > before {
            continue;
        }
        if let Some(write) = stmt.as_local_variable_write_node()
            && String::from_utf8_lossy(write.name().as_slice()) == name
        {
            found = Some(write.value());
            continue;
        }
        if local_mutated(name, stmt) {
            return None;
        }
    }
    found
}

/// One pass of [`splat_local_elements`] over a statement list. `false` means
/// the local was touched in a way this replay cannot follow.
fn replay_local_array<'a, 'b>(
    name: &str,
    stmts: impl Iterator<Item = &'b Node<'a>>,
    limit: usize,
    acc: &mut Option<Vec<Node<'a>>>,
) -> bool
where
    'a: 'b,
{
    for stmt in stmts {
        // Statements from the directive onward are not part of the value it
        // reads -- including the directive itself, and its own siblings when
        // the walk descends into the `if` that encloses it.
        if stmt.location().start_offset() >= limit {
            continue;
        }
        // A guard this stage decides contributes the branch it selects, the
        // same way `lower_one_class_body_stmt` lowers it.
        if let Some(if_node) = stmt.as_if_node()
            && let Some(cond) = super::defs::static_guard(&if_node.predicate())
        {
            let chosen = if cond {
                if_node.statements().map(|s| s.as_node())
            } else {
                if_node.subsequent()
            };
            if !replay_branch(name, chosen, limit, acc) {
                return false;
            }
            continue;
        }
        if let Some(unless_node) = stmt.as_unless_node()
            && let Some(cond) = super::defs::static_guard(&unless_node.predicate())
        {
            let chosen = if cond {
                unless_node.else_clause().map(|e| e.as_node())
            } else {
                unless_node.statements().map(|s| s.as_node())
            };
            if !replay_branch(name, chosen, limit, acc) {
                return false;
            }
            continue;
        }
        match local_array_op(name, stmt) {
            Some(LocalArrayOp::Assign(elems)) => *acc = Some(elems),
            // An append before any assignment means the array came from
            // somewhere this replay never saw.
            Some(LocalArrayOp::Append(elems)) => match acc.as_mut() {
                Some(list) => list.extend(elems),
                None => return false,
            },
            None if local_mutated(name, stmt) => return false,
            None => {}
        }
    }
    true
}

/// [`replay_local_array`] over the branch a folded guard selected -- a
/// `StatementsNode`, a final `else`, a nested `elsif`, or nothing.
fn replay_branch<'a>(
    name: &str,
    chosen: Option<Node<'a>>,
    limit: usize,
    acc: &mut Option<Vec<Node<'a>>>,
) -> bool {
    let Some(node) = chosen else { return true };
    if let Some(stmts) = node.as_statements_node() {
        let body: Vec<Node<'a>> = stmts.body().iter().collect();
        return replay_local_array(name, body.iter(), limit, acc);
    }
    if let Some(else_node) = node.as_else_node() {
        let body: Vec<Node<'a>> = else_node
            .statements()
            .map(|s| s.body().iter().collect())
            .unwrap_or_default();
        return replay_local_array(name, body.iter(), limit, acc);
    }
    replay_local_array(name, std::iter::once(&node), limit, acc)
}

/// What a statement does to the local array being replayed.
enum LocalArrayOp<'a> {
    /// `members = [...]` -- the list starts over from these elements.
    Assign(Vec<Node<'a>>),
    /// `members.push(...)` / `members << x` / `members.concat([...])` /
    /// `members += [...]` -- these elements go on the end.
    Append(Vec<Node<'a>>),
}

fn local_array_op<'a>(name: &str, stmt: &Node<'a>) -> Option<LocalArrayOp<'a>> {
    if let Some(write) = stmt.as_local_variable_write_node() {
        if String::from_utf8_lossy(write.name().as_slice()) != name {
            return None;
        }
        return Some(LocalArrayOp::Assign(
            write.value().as_array_node()?.elements().iter().collect(),
        ));
    }
    // `members += [...]` is an operator write, not a call on the local.
    if let Some(op) = stmt.as_local_variable_operator_write_node() {
        if String::from_utf8_lossy(op.name().as_slice()) != name
            || op.binary_operator().as_slice() != b"+"
        {
            return None;
        }
        return Some(LocalArrayOp::Append(
            op.value().as_array_node()?.elements().iter().collect(),
        ));
    }
    let call = stmt.as_call_node()?;
    if local_read_name(&call.receiver()?).as_deref() != Some(name) {
        return None;
    }
    let args: Vec<Node<'a>> = call
        .arguments()
        .map(|a| a.arguments().iter().collect())
        .unwrap_or_default();
    match call.name().as_slice() {
        b"push" | b"append" => Some(LocalArrayOp::Append(args)),
        b"<<" => match args.as_slice() {
            [_] => Some(LocalArrayOp::Append(args)),
            _ => None,
        },
        b"concat" => match args.as_slice() {
            [one] => Some(LocalArrayOp::Append(
                one.as_array_node()?.elements().iter().collect(),
            )),
            _ => None,
        },
        _ => None,
    }
}

/// The name a plain local-variable READ spells, or `None` for any other node.
fn local_read_name(node: &Node<'_>) -> Option<String> {
    let read = node.as_local_variable_read_node()?;
    Some(String::from_utf8_lossy(read.name().as_slice()).into_owned())
}

/// Whether a statement names this local ANYWHERE -- the poison test that keeps
/// Whether a statement REBINDS or MUTATES this local -- the poison test that
/// keeps a build step the replay cannot follow from silently shortening a
/// value. A plain READ is not poison: a library declares nine functions with
/// the same `str_suffix` in each of them, and every one of those reads is
/// still the value the assignment gave it.
fn local_mutated(name: &str, stmt: &Node<'_>) -> bool {
    /// Receiver-position calls that change the object in place. Any `!`
    /// method counts too; everything else leaves the value alone.
    const MUTATORS: &[&str] = &[
        "<<",
        "push",
        "append",
        "concat",
        "replace",
        "insert",
        "prepend",
        "clear",
        "[]=",
        "pop",
        "shift",
        "unshift",
        "delete",
        "delete_at",
        "delete_if",
        "keep_if",
        "fill",
        "force_encoding",
    ];
    struct Search<'n> {
        name: &'n str,
        found: bool,
    }
    impl Search<'_> {
        fn hit(&mut self, name: &ruby_prism::ConstantId) {
            self.found |= String::from_utf8_lossy(name.as_slice()) == self.name;
        }
    }
    impl<'pr> ruby_prism::Visit<'pr> for Search<'_> {
        fn visit_local_variable_write_node(
            &mut self,
            node: &ruby_prism::LocalVariableWriteNode<'pr>,
        ) {
            self.hit(&node.name());
            self.visit(&node.value());
        }
        fn visit_local_variable_target_node(
            &mut self,
            node: &ruby_prism::LocalVariableTargetNode<'pr>,
        ) {
            self.hit(&node.name());
        }
        fn visit_local_variable_operator_write_node(
            &mut self,
            node: &ruby_prism::LocalVariableOperatorWriteNode<'pr>,
        ) {
            self.hit(&node.name());
            self.visit(&node.value());
        }
        fn visit_local_variable_and_write_node(
            &mut self,
            node: &ruby_prism::LocalVariableAndWriteNode<'pr>,
        ) {
            self.hit(&node.name());
            self.visit(&node.value());
        }
        fn visit_local_variable_or_write_node(
            &mut self,
            node: &ruby_prism::LocalVariableOrWriteNode<'pr>,
        ) {
            self.hit(&node.name());
            self.visit(&node.value());
        }
        fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
            if let Some(recv) = node.receiver()
                && local_read_name(&recv).as_deref() == Some(self.name)
            {
                let method = String::from_utf8_lossy(node.name().as_slice()).into_owned();
                self.found |= method.ends_with('!') || MUTATORS.contains(&method.as_str());
            }
            if let Some(recv) = node.receiver() {
                self.visit(&recv);
            }
            if let Some(args) = node.arguments() {
                self.visit(&args.as_node());
            }
            if let Some(block) = node.block() {
                self.visit(&block);
            }
        }
    }
    use ruby_prism::Visit as _;
    let mut search = Search { name, found: false };
    search.visit(stmt);
    search.found
}

/// Flatten a constant reference (`FFI`, `FFI::Library`, `FFI::Library::LIBC`) to
/// its `::`-joined spelling, or `None` if it isn't a plain constant path.
pub(crate) fn const_path_string(node: &Node<'_>) -> Option<String> {
    if let Some(c) = node.as_constant_read_node() {
        return Some(String::from_utf8_lossy(c.name().as_slice()).into_owned());
    }
    let path = node.as_constant_path_node()?;
    let name = String::from_utf8_lossy(path.name()?.as_slice()).into_owned();
    match path.parent() {
        Some(parent) => Some(format!("{}::{name}", const_path_string(&parent)?)),
        None => Some(name),
    }
}

/// Lower one directive inside an FFI-library module. Returns `true` if it WAS an
/// FFI directive (`ffi_lib` / `attach_function`), `false` to fall through to the
/// ordinary class-body lowering.
pub(crate) fn lower_ffi_directive(
    result: &ParseResult,
    hir: &mut Hir,
    node: &Node<'_>,
    ffi_lib: &mut crate::hir::FfiLib,
    aliases: &mut crate::compiler::FMap<String, crate::hir::FfiType>,
    out: &mut Vec<NodeId>,
    class_body: &[Node<'_>],
) -> PResult<bool> {
    // Catch up on types declared since this body's snapshot: a struct or
    // typedef declared by a NESTED class body mid-module (sha3 nests its
    // state struct inside the library module, above the `attach_function`s
    // that pass it).
    for (k, v) in hir.inherited_ffi_types() {
        match aliases.get(&k) {
            // A by-reference placeholder YIELDS to a real layout. A struct
            // class is known before its `layout` lowers -- so a body seeded
            // when only the name existed held `StructRef`, and a plain
            // `or_insert` could never replace it, which made a `.by_value`
            // further down the same body report a layout it now has.
            Some(crate::hir::FfiType::StructRef(_))
                if matches!(v, crate::hir::FfiType::Struct(_)) => {}
            Some(_) => continue,
            None => {}
        }
        aliases.insert(k, v);
    }
    // `SassTag = enum(:sass_boolean, :sass_number, ...)` -- the ANONYMOUS enum,
    // named by the constant it is assigned to rather than by a `:tag` argument.
    // sassc and google-protobuf both declare every one of their enums this way,
    // and then use the constant as a field/argument type. The gem's own
    // disambiguation: a leading symbol FOLLOWED BY an array is the NAMED form
    // (`Tag = enum :tag, [members]`), registered under the tag AND the
    // constant; any other shape reads every argument as a member.
    if let Some(write) = node.as_constant_write_node()
        && let Some(call) = write.value().as_call_node()
        && call.receiver().is_none()
        && call.name().as_slice() == b"enum"
    {
        let args: Vec<Node<'_>> = call
            .arguments()
            .map(|a| a.arguments().iter().collect())
            .unwrap_or_default();
        let name = String::from_utf8_lossy(write.name().as_slice()).into_owned();
        let (tag, ty) = enum_declaration(result, hir, &args, out, class_body)?;
        if let Some(tag) = tag {
            hir.declare_ffi_type(&tag, &ty);
            aliases.insert(tag, ty.clone());
        }
        hir.declare_ffi_type(&name, &ty);
        aliases.insert(name, ty);
        // Consumed, like every other declaration here: the enum is a TYPE, and
        // zeo has no `FFI::Enum` object to bind the constant to. A program that
        // reads the constant at runtime gets a NameError -- loud, not wrong.
        return Ok(true);
    }
    let Some(call) = node.as_call_node() else {
        return Ok(false);
    };
    // `FFI.add_typedef(:uint32, :OM_uint32)` is `typedef` spelled on the FFI
    // module itself, same argument order. gssapi declares its whole C type
    // vocabulary that way, in a file above the structs that use it -- which the
    // program-wide table (`Hir::ffi_types`) is what makes reachable.
    let on_ffi_module = call
        .receiver()
        .and_then(|r| const_path_string(&r))
        .is_some_and(|p| p == "FFI" || p == "::FFI");
    if call.receiver().is_some() && !(on_ffi_module && call.name().as_slice() == b"add_typedef") {
        return Ok(false);
    }
    let args: Vec<Node<'_>> = call
        .arguments()
        .map(|a| a.arguments().iter().collect())
        .unwrap_or_default();
    match call.name().as_slice() {
        b"add_typedef" => {
            if args.len() != 2 {
                return Err(format!(
                    "FFI.add_typedef expects 2 arguments (existing_type, new_name), got {}",
                    args.len()
                )
                .into());
            }
            let existing = ffi_type_node(&args[0], aliases, TypePos::Signature)?;
            let new_name = ffi_symbol_str(&args[1])?;
            hir.declare_ffi_type(&new_name, &existing);
            aliases.insert(new_name, existing);
            Ok(true)
        }
        b"ffi_lib" => {
            // `ffi_lib "m"` / `ffi_lib FFI::Library::LIBC` / a candidate list.
            // The most-recently declared library links every subsequent
            // `attach_function`.
            if !args.is_empty() {
                // `ffi_lib FFI::CURRENT_PROCESS`: the symbols come from the
                // already-linked image, which is exactly what a `lib` of
                // `None` emits -- an extern block with no `#[link]`.
                if names_current_process(&args) {
                    *ffi_lib = crate::hir::FfiLib::None;
                } else if let Some(lib) = ffi_lib_name(&args, hir) {
                    *ffi_lib = lib;
                } else {
                    // NOTHING folds (an ENV read, a local, a helper call):
                    // evaluate the candidate expressions when the class body
                    // EXECUTES -- `__zeo_ffi_lib` flattens the values and
                    // dlopens eagerly, so an unopenable library is CRuby's
                    // require-time `LoadError` at this very statement -- and
                    // every following `attach_function` resolves its symbol
                    // from the slot's handle.
                    let slot = hir.ffi_lib_slots;
                    hir.ffi_lib_slots += 1;
                    let slot_lit = hir.push(HirNode::IntegerLit(slot as i64));
                    let mut call_args = vec![crate::hir::ArrayElem::Single(slot_lit)];
                    // Each argument rides as a (splat?, expr) pair: the gem
                    // loads EVERY top-level argument as its own library (an
                    // Array value lists alternatives for one), so a splat
                    // must expand back into separate values -- codegen reads
                    // the flag and spreads the evaluated array.
                    for a in &args {
                        let (splatted, id) = match a.as_splat_node() {
                            Some(s) => {
                                let inner = s.expression().ok_or_else(|| {
                                    "ffi_lib can't forward a bare `*` splat (zeo limitation)"
                                        .to_string()
                                })?;
                                (1, super::lower_node(result, hir, &inner)?)
                            }
                            None => (0, super::lower_node(result, hir, a)?),
                        };
                        let flag = hir.push(HirNode::IntegerLit(splatted));
                        call_args.push(crate::hir::ArrayElem::Single(flag));
                        call_args.push(crate::hir::ArrayElem::Single(id));
                    }
                    out.push(hir.push(HirNode::Call {
                        receiver: None,
                        name: "__zeo_ffi_lib".to_string(),
                        args: call_args,
                        kwargs: Vec::new(),
                        block: None,
                        block_arg: None,
                        safe: false,
                    }));
                    *ffi_lib = crate::hir::FfiLib::Deferred { slot };
                }
            }
            Ok(true)
        }
        b"typedef" => {
            // `typedef :existing, :alias` -- register a type alias resolvable by
            // every subsequent `attach_function`. The gem requires the definition
            // precede its use, which source-order iteration gives us for free.
            if args.len() != 2 {
                return Err(format!(
                    "typedef expects 2 arguments (existing_type, new_name), got {}",
                    args.len()
                )
                .into());
            }
            let existing = ffi_type_node(&args[0], aliases, TypePos::Signature)?;
            let new_name = ffi_symbol_str(&args[1])?;
            hir.declare_ffi_type(&new_name, &existing);
            aliases.insert(new_name, existing);
            Ok(true)
        }
        b"enum" => {
            // `enum :tag, [:sym, val, :sym, ...]` -- register `:tag` as an enum
            // type usable in a later type list. (A bare `enum [...]` statement,
            // whose members become module values with no type name at all, is
            // still a follow-on; the constant-assigned form is handled above.)
            // A NAMELESS `enum [:a, :b]` statement registers no type name, so
            // nothing later can reference it in a type position; its one
            // effect -- symbol/int conversion for arguments typed with THAT
            // enum -- is unreachable without a name. Validate the members and
            // consume the statement.
            if let [only] = args.as_slice()
                && only.as_array_node().is_some()
            {
                parse_enum_members(std::slice::from_ref(only), hir, out, class_body)?;
                return Ok(true);
            }
            let (tag, ty) = enum_declaration(result, hir, &args, out, class_body)?;
            let Some(tag) = tag else {
                return Err("enum expects `:tag, [members]` or `[members]`"
                    .to_string()
                    .into());
            };
            hir.declare_ffi_type(&tag, &ty);
            aliases.insert(tag, ty);
            Ok(true)
        }
        b"callback" => {
            // `callback :tag, [arg_types], ret_type` -- register `:tag` as a C
            // function-pointer type, carrying its full signature so a Ruby Proc
            // passed for a `:tag` argument can be marshaled into a libffi closure
            // (see codegen's `ffi_marshal_in`).
            let (tag, params, ret) = match (args.first(), args.get(1), args.get(2)) {
                (Some(t), Some(p), Some(r)) if t.as_symbol_node().is_some() => {
                    (ffi_symbol_str(t)?, p, r)
                }
                _ => {
                    return Err("callback expects `:tag, [arg_types], return_type`"
                        .to_string()
                        .into());
                }
            };
            let arg_types = ffi_type_array(params, aliases, class_body)?;
            let ret_ty = ffi_type_node(ret, aliases, TypePos::Signature)?;
            // `:strptr` is an attach_function RETURN device; a callback's CIF
            // marshals through plain kinds and has no pair wrap.
            if arg_types
                .iter()
                .chain(std::iter::once(&ret_ty))
                .any(|t| matches!(t, crate::hir::FfiType::StrPtr))
            {
                return Err(
                    "`:strptr` is only usable as an `attach_function` return type"
                        .to_string()
                        .into(),
                );
            }
            if arg_types
                .iter()
                .chain(std::iter::once(&ret_ty))
                .any(|t| matches!(t, crate::hir::FfiType::Struct(_)))
            {
                return Err(
                    "a callback signature can't pass a struct BY VALUE (zeo limitation) -- \
                     use `.by_ref`"
                        .to_string()
                        .into(),
                );
            }
            let mut arg_types = arg_types;
            degrade_callback_struct_refs(&mut arg_types, &ret_ty)?;
            let ty = crate::hir::FfiType::Callback(arg_types, Box::new(ret_ty));
            hir.declare_ffi_type(&tag, &ty);
            aliases.insert(tag, ty);
            Ok(true)
        }
        b"attach_function" => {
            out.push(lower_attach_function(
                result,
                hir,
                &args,
                ffi_lib.clone(),
                aliases,
                class_body,
            )?);
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// `[:ok, 0, :busy, 3, :error]` -> `[("ok",0),("busy",3),("error",4)]`. Members
/// are symbols, each optionally followed by an explicit integer value; an
/// omitted value auto-increments from the previous (starting at 0), exactly as
/// the `ffi` gem's `enum` does. A value may also be a constant this class body
/// already set to an integer (`ffi_const_int`).
/// One `enum` declaration, in either tier: `(tag, type)`.
///
/// The gem decides the shape by RUNTIME CLASS -- `Library#enum` takes the
/// named form when `args[0]` is a Symbol and `args[1]` is an Array, and reads
/// every argument as a member otherwise. Syntax can only approximate that, and
/// the approximation this makes is the one that matches: exactly two
/// arguments, the first a literal symbol, the second a single unsplatted
/// expression. A splat cannot be it -- `enum(:level, *levels)` passes the tag
/// as `args[0]` and the members as `args[1..]`, so `args[1]` is a member, not
/// an Array, and the gem reads the whole list anonymously (oracle-checked: a
/// signature naming `:level` then raises "unable to resolve type").
///
/// When the members fold, the members ARE the type. When they don't -- a
/// helper call, a value read out of a shared library -- the declaration
/// defers: the ABI is `int` either way, so only the marshaling table waits for
/// the class body to run. See [`crate::hir::FfiType::EnumSlot`].
fn enum_declaration<'a>(
    result: &ParseResult,
    hir: &mut Hir,
    args: &[Node<'a>],
    out: &mut Vec<NodeId>,
    class_body: &[Node<'a>],
) -> PResult<(Option<String>, crate::hir::FfiType)> {
    let named = match args {
        [tag, members] if tag.as_symbol_node().is_some() && members.as_splat_node().is_none() => {
            Some((ffi_symbol_str(tag)?, members))
        }
        _ => None,
    };
    let (tag, member_nodes) = match &named {
        Some((tag, members)) => (Some(tag.clone()), std::slice::from_ref(*members)),
        None => (None, args),
    };
    if let Ok(members) = parse_enum_members(member_nodes, hir, out, class_body) {
        return Ok((tag, crate::hir::FfiType::Enum(members)));
    }
    let slot = hir.ffi_enum_slots;
    hir.ffi_enum_slots += 1;
    let slot_lit = hir.push(HirNode::IntegerLit(slot as i64));
    let mut call_args = vec![crate::hir::ArrayElem::Single(slot_lit)];
    for m in member_nodes {
        // A splat needs no marker: `enum_store` flattens every argument, which
        // is also what `easy_options(:enum).to_a.flatten` relies on.
        let id = match m.as_splat_node() {
            Some(s) => {
                let inner = s.expression().ok_or_else(|| {
                    "enum can't forward a bare `*` splat (zeo limitation)".to_string()
                })?;
                super::lower_node(result, hir, &inner)?
            }
            None => super::lower_node(result, hir, m)?,
        };
        call_args.push(crate::hir::ArrayElem::Single(id));
    }
    out.push(hir.push(HirNode::Call {
        receiver: None,
        name: "__zeo_ffi_enum".to_string(),
        args: call_args,
        kwargs: Vec::new(),
        block: None,
        block_arg: None,
        safe: false,
    }));
    Ok((tag, crate::hir::FfiType::EnumSlot(slot)))
}

fn parse_enum_members<'a>(
    args: &[Node<'a>],
    hir: &Hir,
    body_so_far: &[NodeId],
    class_body: &[Node<'a>],
) -> PResult<Vec<(String, i64)>> {
    // NOT splat-expanded: `enum(:level, *levels)` is the gem's ANONYMOUS
    // form (every argument is a member, `:level` included), not a named enum
    // over the array -- oracle-checked, it raises "unable to resolve type
    // 'level'" at the first signature that names the tag.
    //
    // The members are either one literal array or the argument list itself --
    // `enum :tag, [:a, :b]` and `enum(:a, :b)` both reach here.
    let unwrapped: Vec<Node<'a>>;
    let elems: &[Node<'a>] = match args {
        [one] if word_list_to_syms(one).is_some() => {
            unwrapped = word_list_to_syms(one).expect("just matched");
            &unwrapped
        }
        // `enum :colour, members` -- the list is a body-local the class body
        // built up, the same value `layout(*members)` reads.
        [one]
            if local_read_name(one)
                .and_then(|n| local_array_elements(&n, class_body, one.location().start_offset()))
                .is_some() =>
        {
            unwrapped = local_read_name(one)
                .and_then(|n| local_array_elements(&n, class_body, one.location().start_offset()))
                .expect("just matched");
            &unwrapped
        }
        [one] if one.as_array_node().is_some() => {
            unwrapped = one
                .as_array_node()
                .expect("just matched")
                .elements()
                .iter()
                .collect();
            &unwrapped
        }
        _ => args,
    };
    if elems.is_empty() {
        return Err("enum expects at least one member".to_string().into());
    }
    let mut out: Vec<(String, i64)> = Vec::new();
    let mut next = 0i64;
    let mut i = 0;
    while i < elems.len() {
        let name = ffi_symbol_str(&elems[i])?;
        i += 1;
        // The next element is a VALUE unless it reads as a member name (a
        // literal symbol/string). Deciding by shape first keeps the error
        // honest: an unfoldable value used to be re-read as the next member
        // and rejected as "expected a literal symbol", naming the wrong rule.
        let value = match elems.get(i) {
            Some(n) if n.as_symbol_node().is_none() && n.as_string_node().is_none() => {
                i += 1;
                ffi_const_int(n, hir, body_so_far).ok_or_else(|| {
                    format!(
                        "enum member `{name}`'s value must be an integer literal or a \
                         constant already set to one (zeo limitation)"
                    )
                })?
            }
            _ => next,
        };
        out.push((name, value));
        next = value + 1;
    }
    Ok(out)
}

/// A compile-time INTEGER in an FFI declaration position (`0`, `(1 << 5)`,
/// `FFI::Type::LONG.size`, a constant this class body already set to one), or
/// `None` if the node isn't one (i.e. the next member symbol, or the list's
/// end). Enum member values and inline-array counts both fold through here:
/// an FFI declaration's integers decide marshaling tables and field offsets,
/// so they are needed at LOWERING time -- this is deliberately not a general
/// constant folder.
pub(crate) fn ffi_const_int(node: &Node<'_>, hir: &Hir, body_so_far: &[NodeId]) -> Option<i64> {
    if let Some(n) = body_const_int(hir, body_so_far, node) {
        return Some(n);
    }
    enum_int_literal(node, hir, body_so_far)
}

fn enum_int_literal(node: &Node<'_>, hir: &Hir, body_so_far: &[NodeId]) -> Option<i64> {
    if let Some(int) = node.as_integer_node() {
        let value = int.value();
        let (negative, digits) = value.to_u32_digits();
        return assemble_i64(negative, digits);
    }
    // `(1 << 0)` -- flag enums are written as shifts and ors far more often
    // than as the numbers they come to, and the value is a compile-time
    // constant either way. gir_ffi's `enum :IRepositoryLoadFlags, [:LAZY, (1 <<
    // 0)]` is the case. Folded here rather than left to a general constant
    // folder because an enum member's value is needed at LOWERING time: it is
    // what the generated marshaling tables are built from.
    if let Some(paren) = node.as_parentheses_node()
        && let Some(stmts) = paren.body()
        && let Some(stmts) = stmts.as_statements_node()
    {
        let only: Vec<Node<'_>> = stmts.body().iter().collect();
        if let [inner] = only.as_slice() {
            return ffi_const_int(inner, hir, body_so_far);
        }
        return None;
    }
    let call = node.as_call_node()?;
    let recv = call.receiver()?;
    let args: Vec<Node<'_>> = call
        .arguments()
        .map(|a| a.arguments().iter().collect())
        .unwrap_or_default();
    // `::FFI::Type::LONG.size` -- a scalar's byte width, a constant of the
    // target. dcu-typhoeus sizes its fd_set inline array with it.
    if call.name().as_slice() == b"size"
        && args.is_empty()
        && let Some(path) = const_path_string(&recv)
        && let Some(leaf) = path.trim_start_matches("::").strip_prefix("FFI::Type::")
        && let Some(s) = zeo_abi::ffi::CScalar::from_type_constant(leaf)
        && !matches!(s, zeo_abi::ffi::CScalar::Void)
    {
        return Some(s.size() as i64);
    }
    // `BreakdownStepStruct.size * MAX_TIERS` -- an EARLIER struct's extent,
    // which zeo laid out itself, so it is as much a compile-time constant as
    // the literal the gem could have written. j-law-ruby sizes its inline
    // storage arrays this way. `.alignment` for the same reason.
    if args.is_empty()
        && matches!(call.name().as_slice(), b"size" | b"alignment")
        && let Some(path) = const_path_string(&recv)
        && let Some(layout) = hir
            .ffi_struct_layouts
            .get(path.rsplit("::").next().unwrap_or(&path))
    {
        let n = match call.name().as_slice() {
            b"size" => layout.size,
            _ => layout.align,
        };
        return i64::try_from(n).ok();
    }
    let lhs = ffi_const_int(&recv, hir, body_so_far)?;
    match (call.name().as_slice(), args.as_slice()) {
        (b"-@", []) => lhs.checked_neg(),
        (b"~", []) => Some(!lhs),
        (op, [rhs]) => {
            let rhs = ffi_const_int(rhs, hir, body_so_far)?;
            match op {
                b"<<" => u32::try_from(rhs).ok().and_then(|s| lhs.checked_shl(s)),
                b">>" => u32::try_from(rhs).ok().and_then(|s| lhs.checked_shr(s)),
                b"|" => Some(lhs | rhs),
                b"&" => Some(lhs & rhs),
                b"^" => Some(lhs ^ rhs),
                b"+" => lhs.checked_add(rhs),
                b"-" => lhs.checked_sub(rhs),
                b"*" => lhs.checked_mul(rhs),
                b"/" if rhs != 0 => lhs.checked_div(rhs),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Recognize an FFI `layout :name, :type, :name, :type, ...` directive inside a
/// `class < FFI::Struct` body and return its `(field, type)` pairs, or `None`
/// if `node` isn't a `layout` call.
/// One declared field: name, type, and the byte offset the declaration PINNED
/// it to, if it named one. See [`as_ffi_layout`].
pub(crate) type FfiField = (String, crate::hir::FfiType, Option<usize>);

pub(crate) fn as_ffi_layout<'a>(
    node: &Node<'a>,
    aliases: &crate::compiler::FMap<String, crate::hir::FfiType>,
    hir: &Hir,
    body_so_far: &[NodeId],
    class_body: &[Node<'a>],
) -> PResult<Option<Vec<FfiField>>> {
    let Some(call) = node.as_call_node() else {
        return Ok(None);
    };
    if call.receiver().is_some() || call.name().as_slice() != b"layout" {
        return Ok(None);
    }
    let args: Vec<Node<'a>> = call
        .arguments()
        .map(|a| a.arguments().iter().collect())
        .unwrap_or_default();
    // `layout(*members)` -- the field list is a body-local array the class
    // body built up. Reading it back is what lets a conditionally-shaped
    // struct lower at all.
    let args = match splat_local_elements(&args, class_body, node) {
        Some(elems) => elems,
        None => args,
    };
    let field =
        |name_node: &Node<'_>, ty_node: &Node<'_>| -> PResult<(String, crate::hir::FfiType)> {
            let name = ffi_symbol_str(name_node)?;
            let ty = match layout_array_type(ty_node, aliases, hir, body_so_far)? {
                Some(t) => t,
                // A body constant holding a type symbol resolves to what it
                // names; anything else takes the ordinary type-node path.
                None => match body_const_type_symbol(hir, body_so_far, ty_node) {
                    Some(sym) => ffi_type_of(&sym, aliases)?,
                    None => ffi_type_node(ty_node, aliases, TypePos::Field)?,
                },
            };
            // A TYPEDEF'D struct name resolves through `find_type` to the
            // by-reference wrapper, so as a field it is a plain pointer.
            // A bare struct CLASS written here means embed INLINE -- which
            // needs the layout zeo never saw (a DSL-built one), so it stays
            // a loud rejection rather than a silently mis-shaped field.
            let ty = match ty {
                crate::hir::FfiType::StructRef(path) => {
                    if const_path_string(ty_node).is_some() {
                        return Err(format!(
                            "`{path}`'s layout isn't known to zeo (its `layout` directive \
                             never lowered), so it can't be embedded inline as a field"
                        )
                        .into());
                    }
                    crate::hir::FfiType::Pointer
                }
                t => t,
            };
            Ok((name, ty))
        };
    // The gem's documented alternative spellings: one hash instead of a flat
    // pair list -- `layout(magic: :uint32)` (a trailing keyword hash) and
    // `layout({ :dwId => :uint })` (a braced one).
    if args.len() == 1
        && let Some(pairs) = hash_pairs(&args[0])
    {
        let mut fields = Vec::new();
        for (k, v) in &pairs {
            let (name, ty) = field(k, v)?;
            fields.push((name, ty, None));
        }
        return Ok(Some(fields));
    }
    if args.is_empty() {
        return Err("FFI::Struct `layout` expects `:name, :type` pairs"
            .to_string()
            .into());
    }
    let mut fields = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let Some(ty_node) = args.get(i + 1) else {
            return Err("FFI::Struct `layout` expects `:name, :type` pairs"
                .to_string()
                .into());
        };
        let (name, ty) = field(&args[i], ty_node)?;
        i += 2;
        // The gem's THIRD element per field: an explicit byte offset
        // (`layout :Type, :int16, 0, :Size, :int32, 2` -- every Win32 header
        // struct in winwindow). Decided by SHAPE first, like an enum member's
        // value: a name is a symbol or a string, so anything else in that
        // slot is the offset, and a non-foldable one is an honest rejection
        // rather than a mis-read field name.
        let offset =
            match args.get(i) {
                Some(n) if n.as_symbol_node().is_none() && n.as_string_node().is_none() => {
                    i += 1;
                    let off = ffi_const_int(n, hir, body_so_far).ok_or_else(|| {
                        format!(
                            "field `{name}`'s explicit offset must be an integer literal, or a \
                         constant already set to one (zeo limitation)"
                        )
                    })?;
                    Some(usize::try_from(off).map_err(|_| {
                        format!("field `{name}`'s explicit offset can't be negative")
                    })?)
                }
                _ => None,
            };
        fields.push((name, ty, offset));
    }
    Ok(Some(fields))
}

/// A hash literal's `(key, value)` node pairs -- braced or keyword form. Any
/// non-pair element (a `**splat`) declines the whole hash.
fn hash_pairs<'a>(node: &Node<'a>) -> Option<Vec<(Node<'a>, Node<'a>)>> {
    let elements: Vec<Node<'a>> = if let Some(h) = node.as_hash_node() {
        h.elements().iter().collect()
    } else if let Some(h) = node.as_keyword_hash_node() {
        h.elements().iter().collect()
    } else {
        return None;
    };
    elements
        .into_iter()
        .map(|e| e.as_assoc_node().map(|a| (a.key(), a.value())))
        .collect()
}

/// `[:uint8, 384]` in a layout TYPE position -- an inline array of 384 bytes
/// stored in place. `None` when the node isn't an array literal at all.
///
/// The element count has to be DECIDABLE: it fixes every following field's
/// offset, so a count zeo cannot read would be a wrong struct rather than a
/// slower one. An integer literal, or a constant this class body already
/// assigned an integer -- sys-filesystem's `UUID_NODE_LEN = 6` two lines above
/// its `layout(...)` is the shape, and C bindings spell array widths that way
/// far more often than not.
fn layout_array_type(
    node: &Node<'_>,
    aliases: &crate::compiler::FMap<String, crate::hir::FfiType>,
    hir: &Hir,
    body_so_far: &[NodeId],
) -> PResult<Option<crate::hir::FfiType>> {
    let Some(array) = node.as_array_node() else {
        return Ok(None);
    };
    let elems: Vec<Node<'_>> = array.elements().iter().collect();
    if elems.len() != 2 {
        return Err(
            "an inline array field is written `[element_type, count]` (zeo limitation)"
                .to_string()
                .into(),
        );
    }
    // The element type takes the same body-constant fold as a plain field
    // (`[WCHAR_T, CCHARW_MAX]` -- both halves are constants in ffi-ncurses),
    // and may itself be an inline array: X11's `XTransform` is `layout
    // :matrix, [[:XFixed, 3], 3]`, a 3x3 matrix of fixed-point values. C lays
    // a 2-D array out as rows of rows, which is exactly this nesting.
    let elem = match layout_array_type(&elems[0], aliases, hir, body_so_far)? {
        Some(inner) => inner,
        None => match body_const_type_symbol(hir, body_so_far, &elems[0]) {
            Some(sym) => ffi_type_of(&sym, aliases)?,
            None => ffi_type_node(&elems[0], aliases, TypePos::Field)?,
        },
    };
    // Same rule as a plain field's: a typedef'd struct name is the
    // by-reference wrapper (one pointer per element); a bare layout-less
    // struct CLASS would embed inline, which needs the layout.
    let elem = match elem {
        crate::hir::FfiType::StructRef(path) => {
            if const_path_string(&elems[0]).is_some() {
                return Err(format!(
                    "`{path}`'s layout isn't known to zeo (its `layout` directive never \
                     lowered), so it can't be embedded inline as an array element"
                )
                .into());
            }
            crate::hir::FfiType::Pointer
        }
        t => t,
    };
    let count = ffi_const_int(&elems[1], hir, body_so_far).ok_or_else(|| {
        "an inline array field's element COUNT must be an integer literal, or a constant this \
             class body already set to one -- it decides where every following field starts (zeo \
             limitation)"
            .to_string()
    })?;
    if count < 0 {
        return Err("an inline array field's element count can't be negative"
            .to_string()
            .into());
    }
    Ok(Some(crate::hir::FfiType::Array(
        Box::new(elem),
        count as usize,
    )))
}

/// The integer a bare constant names, read off the `ConstWrite` this class body
/// already lowered for it. Scoped to the body on purpose: a layout's array
/// width is written beside the layout, and reaching further would mean deciding
/// a name against a scope chain that is still being built.
fn body_const_int(hir: &Hir, body_so_far: &[NodeId], node: &Node<'_>) -> Option<i64> {
    let wanted = const_leaf_name(node)?;
    let own = body_so_far.iter().rev().find_map(|&id| match &hir[id] {
        HirNode::ConstWrite { name, value, .. } if *name == wanted => match hir[*value] {
            HirNode::IntegerLit(n) => Some(n),
            _ => None,
        },
        _ => None,
    });
    // An ENCLOSING body's constant (ffi-ncurses spells its counts in the
    // module wrapping the struct) reaches here through the recorded side
    // map -- see `Hir::ffi_int_consts` for the poison rule.
    own.or_else(|| hir.ffi_int_consts.get(&wanted).copied().flatten())
}

/// The leaf name of a bare (`LEN`) or QUALIFIED (`Limits::LEN`) constant read.
/// A qualified read reduces to its leaf on purpose: the side maps these feed
/// (`ffi_int_consts`/`ffi_symbol_consts`) are leaf-keyed with a
/// poison-on-conflict rule, the same reduction the FFI type table applies.
fn const_leaf_name(node: &Node<'_>) -> Option<String> {
    let path = const_path_string(node)?;
    Some(path.rsplit("::").next().unwrap_or(&path).to_string())
}

/// A body constant holding a type SYMBOL (`NCURSES_ATTR_T = :int` above a
/// `layout :attr, NCURSES_ATTR_T` -- ffi-ncurses spells its whole layout
/// vocabulary this way). The symbol's NAME comes back for the ordinary
/// keyword resolution; `body_const_int`'s sibling, with the same
/// enclosing-body fallback.
fn body_const_type_symbol(hir: &Hir, body_so_far: &[NodeId], node: &Node<'_>) -> Option<String> {
    let wanted = const_leaf_name(node)?;
    let own = body_so_far.iter().rev().find_map(|&id| match &hir[id] {
        HirNode::ConstWrite { name, value, .. } if *name == wanted => match &hir[*value] {
            HirNode::SymbolLit(s) => Some(s.clone()),
            _ => None,
        },
        _ => None,
    });
    own.or_else(|| hir.ffi_symbol_consts.get(&wanted).cloned().flatten())
}

/// The `FFI::MemoryPointer` accessor pair and C layout `(size, align)` for a
/// struct field type. Structs hold scalar/pointer fields; a `:string`/`:bool`/
/// nested-struct field is a clean, greppable rejection (follow-on).
fn ffi_field_accessor(ty: &crate::hir::FfiType) -> PResult<(String, String, usize, usize)> {
    use crate::hir::FfiType::*;
    // An inline array occupies `count` elements IN PLACE, and aligns to one
    // element -- so it is the field that decides where the next one starts.
    // The getter/putter named here are the ELEMENT's, which is what the
    // synthesized proxy indexes with.
    if let Array(elem, count) = ty {
        if matches!(**elem, Callback(..)) {
            return Err(
                "an inline array of callbacks isn't supported yet (zeo limitation)"
                    .to_string()
                    .into(),
            );
        }
        // A struct element has no scalar accessor pair either -- the proxy
        // constructs the element class VIEWING each slot in place (see the
        // `InlineArray` synthesis) -- but its size and alignment place the
        // following fields the same way a scalar's do.
        let (get, put, esize, ealign) = ffi_field_accessor(elem)?;
        return Ok((get, put, esize * count, ealign));
    }
    // A nested struct stored BY VALUE has no scalar accessor pair -- the
    // synthesized reader hands back the struct class VIEWING the field's
    // bytes in place, and the writer copies bytes -- see `Conv::Struct`.
    if let Struct(l) = ty {
        if l.class_path.is_empty() {
            return Err(
                "a nested struct field needs a NAMED struct class (zeo limitation)"
                    .to_string()
                    .into(),
            );
        }
        return Ok((String::new(), String::new(), l.size, l.align));
    }
    // A platform typedef in a FIELD resolves to the width it has on the one
    // target zeo emits for -- see `platform_scalar_of`. A signature can leave
    // it to rustc; an offset cannot wait.
    if let crate::hir::FfiType::PlatformScalar(name) = ty
        && let Some(s) = platform_scalar_of(name)
    {
        return ffi_field_accessor(&crate::hir::FfiType::from(s));
    }
    let (get, put) = match ty {
        Int(w) => (format!("get_int{w}"), format!("put_int{w}")),
        Uint(w) => (format!("get_uint{w}"), format!("put_uint{w}")),
        Float(w) => (format!("get_float{w}"), format!("put_float{w}")),
        // An enum field is a C `int` in memory, a bool a one-byte `_Bool`.
        // Neither reads back as the number it stores; the generated accessor
        // converts -- see `synthesize_ffi_struct`.
        Enum(_) | EnumSlot(_) => ("get_int32".into(), "put_int32".into()),
        Bool => ("get_int8".into(), "put_int8".into()),
        // A `:string` field is a `char *`: read through the pointer, and NOT
        // writable -- CRuby's ffi raises `Cannot set :string fields`, because
        // storing one would need somewhere to keep the bytes alive. A
        // callback field is a C function pointer in memory; the generated
        // accessor wraps/unwraps `FFI::Function` -- see `Conv::Callback`.
        Str | Pointer | Callback(..) => ("get_pointer".into(), "put_pointer".into()),
        other => {
            return Err(format!("FFI::Struct field type `{other:?}` isn't supported yet").into());
        }
    };
    // Width and alignment come from the one shared table -- the same widths
    // codegen's `#[repr(C)]` mirror asserts against.
    let s = ty.c_scalar().expect("the unsupported arms returned above");
    Ok((get, put, s.size(), s.align()))
}

/// A type's spelling in SYNTHESIZED ruby source (an `FFI::Function.new`
/// signature for a callback field): the canonical scalar keyword, with
/// `FFI::Type::VOID` for void -- the keyword table deliberately rejects
/// `:void` in value positions, and the constant resolves to the same kind.
/// A field's `FFI::StructLayout::Field` DESCRIPTOR -- the Field subclass and
/// the type object CRuby answers from `.layout.fields`, built from the same
/// walked layout the accessors are built from.
///
/// Divergence recorded once, here: a `:long` field answers
/// `Type::Builtin::INT64` rather than `LONG`, because both fold to one width
/// before a layout is recorded and the spelling is gone by now.
fn ffi_field_descriptor(name: &str, ty: &crate::hir::FfiType, off: usize) -> PResult<String> {
    use crate::hir::FfiType::*;
    let (size, align) = {
        let (_, _, s, a) = ffi_field_accessor(ty)?;
        (s, a)
    };
    let builtin = |n: &str| format!("::FFI::Type::Builtin::{n}");
    let scalar_name = |ty: &crate::hir::FfiType| match ty {
        Int(w) => Some(format!("INT{w}")),
        Uint(w) => Some(format!("UINT{w}")),
        Float(w) => Some(format!("FLOAT{w}")),
        Bool => Some("BOOL".to_string()),
        Str => Some("STRING".to_string()),
        Pointer => Some("POINTER".to_string()),
        PlatformScalar(n) => platform_scalar_of(n)
            .map(crate::hir::FfiType::from)
            .and_then(|t| match t {
                Int(w) => Some(format!("INT{w}")),
                Uint(w) => Some(format!("UINT{w}")),
                _ => None,
            }),
        _ => None,
    };
    let (class, type_src) = match ty {
        Array(elem, count) => {
            let elem_src = match scalar_name(elem) {
                Some(n) => builtin(&n),
                // An array of structs or of arrays: the element's own
                // descriptor is what `elem_type` should answer with.
                None => match &**elem {
                    Struct(l) => format!("::FFI::StructByValue.new({})", l.class_path),
                    _ => builtin("POINTER"),
                },
            };
            (
                "Array",
                format!("::FFI::ArrayType.new({elem_src}, {count})"),
            )
        }
        Struct(l) => (
            "InnerStruct",
            format!("::FFI::StructByValue.new({})", l.class_path),
        ),
        Callback(args, ret) => {
            let args: Vec<String> = args.iter().map(ruby_ffi_type_src).collect::<PResult<_>>()?;
            (
                "Function",
                format!(
                    "::FFI::FunctionType.new({}, [{}])",
                    ruby_ffi_type_src(ret)?,
                    args.join(", ")
                ),
            )
        }
        Enum(_) | EnumSlot(_) => (
            "Mapped",
            format!("::FFI::Type::Mapped.new({})", builtin("INT32")),
        ),
        Str => ("String", builtin("STRING")),
        Pointer => ("Pointer", builtin("POINTER")),
        other => {
            let n = scalar_name(other)
                .ok_or_else(|| format!("FFI::Struct field type `{other:?}` has no descriptor"))?;
            ("Number", builtin(&n))
        }
    };
    Ok(format!(
        "::FFI::StructLayout::{class}.new(:{name}, {off}, {type_src}, {size}, {align}, self)"
    ))
}

fn ruby_ffi_type_src(ty: &crate::hir::FfiType) -> PResult<String> {
    use crate::hir::FfiType::*;
    Ok(match ty {
        Void => "::FFI::Type::VOID".to_string(),
        // Both enum tiers are an `int` on the wire; a callback trampoline
        // marshals through the raw value either way.
        Enum(_) | EnumSlot(_) => ":int32".to_string(),
        Callback(..) => ":pointer".to_string(),
        Struct(_) | Array(..) => {
            return Err(
                "a callback signature can't pass a struct BY VALUE (zeo limitation) -- \
                 use `.by_ref`"
                    .to_string()
                    .into(),
            );
        }
        // Synthesized source re-lowers in another body, where the alias
        // tables differ -- spell the typedef by its own name (it resolves
        // through the same `PLATFORM_TYPEDEFS` row) and reject the
        // return-only device.
        PlatformScalar(name) => format!(":{name}"),
        StrPtr => {
            return Err(
                "`:strptr` is only usable as an `attach_function` return type"
                    .to_string()
                    .into(),
            );
        }
        scalar => format!(
            ":{}",
            scalar
                .c_scalar()
                .expect("the aggregate arms returned above")
                .keyword()
        ),
    })
}

/// Synthesize the Ruby methods for a `class < FFI::Struct` from its `layout`:
/// `[]`/`[]=` read/write each field at its computed C offset over an owned
/// `FFI::MemoryPointer` ivar, plus `pointer`/`to_ptr`, `size`, `offset_of`, and
/// `members`. Offsets follow C alignment (each field aligned to its own size;
/// total rounded to the max field alignment), matching `ffi 1.17.4` and the C
/// ABI. Returned as source for `parse_and_lower_into`.
/// The two classes an inline array field reads back as, as ruby source.
///
/// Emitted at ABSOLUTE scope (`module ::FFI`) from inside the struct body that
/// first needs them, and only once per program -- redefining them per struct
/// would print a method-redefined warning for every struct after the first.
/// The names are observable (`s[:bytes].class`), so they are the gem's, and so
/// is the split: `CharArray` is the 8-bit one, and the only one with `to_s`.
///
/// Written against `send` on the pointer rather than a per-element-type class,
/// so one pair of classes serves every element width.
const FFI_INLINE_ARRAY_CLASSES: &str = r#"
module ::FFI
  class Struct
    class InlineArray
      include ::Enumerable
      def initialize(__p, __off, __n, __get, __put, __esize, __klass = nil)
        @__p, @__off, @__n, @__get, @__put, @__esize = __p, __off, __n, __get, __put, __esize
        @__klass = __klass
      end
      def size
        @__n
      end
      def [](__i)
        if @__klass
          @__klass.new(@__p + (@__off + __i * @__esize))
        elsif @__get.nil?
          raise ::ArgumentError, "get not supported for FFI::ArrayType"
        else
          @__p.send(@__get, @__off + __i * @__esize)
        end
      end
      def []=(__i, __v)
        if @__klass
          @__p.put_bytes(@__off + __i * @__esize, __v.to_ptr.get_bytes(0, @__esize))
        elsif @__put.nil?
          raise ::ArgumentError, "set not supported for FFI::ArrayType"
        else
          @__p.send(@__put, @__off + __i * @__esize, __v)
        end
      end
      def each
        __i = 0
        while __i < @__n
          yield self[__i]
          __i += 1
        end
        self
      end
      def to_a
        ::Array.new(@__n) { |__i| self[__i] }
      end
      def to_ptr
        @__p + @__off
      end
    end
  end
  class StructLayout
    class CharArray < ::FFI::Struct::InlineArray
      def to_s
        __out = []
        __i = 0
        while __i < @__n
          __b = self[__i]
          break if __b == 0
          __out << (__b & 0xff)
          __i += 1
        end
        __out.pack("C*")
      end
      alias to_str to_s
    end
  end
end
"#;

/// Whether `fields` needs the inline-array proxy classes emitted with them.
pub(crate) fn needs_inline_array_classes(fields: &[FfiField]) -> bool {
    fields
        .iter()
        .any(|(_, t, _)| matches!(t, crate::hir::FfiType::Array(..)))
}

/// The one place field offsets, total size and alignment are computed --
/// consumed by the accessor synthesis below AND recorded as
/// `Hir::ffi_struct_layouts` for by-value passing, so the two views of the
/// same struct cannot disagree.
pub(crate) fn ffi_struct_layout(
    class_path: &str,
    fields: &[FfiField],
    union: bool,
) -> PResult<crate::hir::FfiStructLayout> {
    let round_up = |n: usize, a: usize| -> usize { n.div_ceil(a) * a };
    let mut offset = 0usize;
    let mut max_align = 1usize;
    // A union's members all start at offset 0 and it is as wide as its
    // widest member.
    let mut widest = 0usize;
    let mut placed = Vec::new();
    for (name, ty, pinned) in fields {
        let (_, _, size, align) = ffi_field_accessor(ty)?;
        // A PINNED offset wins outright -- the gem honours the number the
        // declaration gave, however it sits against the field's alignment.
        // Size and alignment still come from the fields themselves, which is
        // why `layout :a, :int16, 0, :b, :int32, 2, :c, :int16, 6` is 8 bytes
        // aligned to 4 and not 8 bytes aligned to 2 (oracle-verified).
        let off = match (union, pinned) {
            (true, _) => 0,
            (false, Some(off)) => *off,
            (false, None) => round_up(offset, align),
        };
        placed.push((name.clone(), ty.clone(), off));
        // The struct's extent, not the last field's end: pinned offsets need
        // not run in order.
        offset = offset.max(off + size);
        widest = widest.max(size);
        max_align = max_align.max(align);
    }
    Ok(crate::hir::FfiStructLayout {
        class_path: class_path.to_string(),
        fields: placed,
        size: round_up(if union { widest } else { offset }, max_align),
        align: max_align,
        union,
    })
}

pub(crate) fn synthesize_ffi_struct(
    layout: &crate::hir::FfiStructLayout,
    with_inline_array_classes: bool,
) -> PResult<String> {
    // How a field's stored bytes become a ruby value and back. Most fields are
    // the number itself.
    enum Conv {
        Plain,
        Enum(Vec<(String, i64)>),
        /// The deferred twin: the table lives in a runtime slot the class body
        /// filled, so the accessor calls through instead of carrying a
        /// literal. ethon's `layout :whatever, :pointer, :code, :easy_code`
        /// stores an enum whose members come from a method.
        EnumSlot(usize),
        Bool,
        Str,
        /// `(class, element count, element size, element struct class)` --
        /// the proxy the field reads back as. `FFI::StructLayout::CharArray`
        /// for an 8-bit element (it is the one that also answers `to_s`),
        /// `FFI::Struct::InlineArray` otherwise, matching the gem. A STRUCT
        /// element carries its class path: the proxy then constructs that
        /// class viewing each slot in place instead of a scalar getter.
        Array(&'static str, usize, usize, Option<String>),
        /// An inline array whose element is ITSELF an inline array -- X11's
        /// `layout :matrix, [[:XFixed, 3], 3]`. The layout is real (C lays a
        /// 2-D array out as rows of rows, and the following fields start
        /// after all of it), but the gem cannot read one back: indexing the
        /// proxy raises `ArgumentError: get not supported for FFI::ArrayType`,
        /// oracle-verified. `(class, element count, row size)`.
        NestedArray(&'static str, usize, usize),
        /// A nested struct stored BY VALUE: `(class path, byte size)`.
        /// Reading yields the class VIEWING the field's bytes in place (a
        /// mutation through the view mutates the parent -- oracle-verified);
        /// writing copies the value's bytes over the field, both exactly as
        /// the gem does.
        Struct(String, usize),
        /// A C function-pointer field: `(ruby arg-type list, ruby return
        /// type)` as source text. Reading wraps the stored address in an
        /// `FFI::Function` (the gem's read class); writing accepts a
        /// pointer/Function as-is or marshals a callable into a Function --
        /// KEPT in an ivar so the closure outlives the write, which is
        /// sturdier than the gem's own keep-it-alive-yourself contract.
        Callback(String, String),
    }
    // (field, getter, putter, offset, conversion) -- offsets come off the
    // recorded layout, whose walk (`ffi_struct_layout`) is the ONE place they
    // are computed.
    let mut placed: Vec<(String, String, String, usize, Conv)> = Vec::new();
    for (name, ty, off) in &layout.fields {
        let (getter, putter, _, _) = ffi_field_accessor(ty)?;
        let conv = match ty {
            crate::hir::FfiType::Enum(m) => Conv::Enum(m.clone()),
            crate::hir::FfiType::EnumSlot(slot) => Conv::EnumSlot(*slot),
            crate::hir::FfiType::Bool => Conv::Bool,
            crate::hir::FfiType::Str => Conv::Str,
            crate::hir::FfiType::Array(elem, count)
                if matches!(**elem, crate::hir::FfiType::Array(..)) =>
            {
                let (_, _, esize, _) = ffi_field_accessor(elem)?;
                Conv::NestedArray("FFI::Struct::InlineArray", *count, esize)
            }
            crate::hir::FfiType::Array(elem, count) => {
                let (_, _, esize, _) = ffi_field_accessor(elem)?;
                let elem_class = match &**elem {
                    crate::hir::FfiType::Struct(l) if l.class_path.is_empty() => {
                        return Err(
                            "an inline array of structs needs a NAMED struct class (zeo limitation)"
                                .to_string()
                                .into(),
                        );
                    }
                    crate::hir::FfiType::Struct(l) => Some(l.class_path.clone()),
                    _ => None,
                };
                let class = if esize == 1 && elem_class.is_none() {
                    "FFI::StructLayout::CharArray"
                } else {
                    "FFI::Struct::InlineArray"
                };
                Conv::Array(class, *count, esize, elem_class)
            }
            crate::hir::FfiType::Struct(l) => Conv::Struct(l.class_path.clone(), l.size),
            crate::hir::FfiType::Callback(cb_args, cb_ret) => {
                let args: Vec<String> = cb_args
                    .iter()
                    .map(ruby_ffi_type_src)
                    .collect::<PResult<_>>()?;
                Conv::Callback(args.join(", "), ruby_ffi_type_src(cb_ret)?)
            }
            _ => Conv::Plain,
        };
        placed.push((name.clone(), getter, putter, *off, conv));
    }
    let total = layout.size;
    let alignment = layout.align;

    // An enum field reads back as its member SYMBOL and accepts either a symbol
    // or the raw integer, which is `Enum#from_native`/`#to_native`. A value with
    // no member keeps its number, exactly as the gem's do.
    let read_arms: String = placed
        .iter()
        .map(|(name, getter, putter, off, conv)| {
            let read = format!("@__ffi_ptr.{getter}({off})");
            let read = match conv {
                Conv::Plain => read,
                Conv::Bool => format!("{read} != 0"),
                Conv::Str => format!("((__p = {read}).null? ? nil : __p.read_string)"),
                // A proxy OVER the struct's own memory, not a copy: writing
                // through it writes the struct, which is what the gem does.
                // A struct element passes its CLASS instead of a getter pair:
                // the proxy then views each slot in place.
                Conv::Array(class, count, esize, elem_class) => match elem_class {
                    Some(ec) => {
                        format!("{class}.new(@__ffi_ptr, {off}, {count}, nil, nil, {esize}, {ec})")
                    }
                    None => format!(
                        "{class}.new(@__ffi_ptr, {off}, {count}, :{getter}, :{putter}, {esize})"
                    ),
                },
                // No accessor pair at all: the proxy answers `size`/`to_ptr`
                // and raises on `[]`, which is the gem's own behaviour for a
                // row-of-rows element.
                Conv::NestedArray(class, count, esize) => {
                    format!("{class}.new(@__ffi_ptr, {off}, {count}, nil, nil, {esize})")
                }
                // The nested class VIEWING the field's bytes in place -- the
                // synthesized `initialize` takes the pointer as-is, so every
                // inner accessor indexes from parent + offset.
                Conv::Struct(class, _) => format!("{class}.new(@__ffi_ptr + {off})"),
                Conv::Callback(args, ret) => {
                    format!("::FFI::Function.new({ret}, [{args}], @__ffi_ptr.get_pointer({off}))")
                }
                Conv::Enum(m) => {
                    let table = m
                        .iter()
                        .map(|(n, v)| format!("{v} => :{n}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{{{table}}}.fetch({read}) {{ |__v| __v }}")
                }
                Conv::EnumSlot(slot) => format!("__zeo_ffi_enum_get({slot}, {read})"),
            };
            format!("        when :{name} then {read}\n")
        })
        .collect();
    let write_arms: String = placed
        .iter()
        .map(|(name, _, putter, off, conv)| {
            // A `:string` field has nowhere to keep the bytes alive, so ruby
            // refuses the write rather than storing a dangling pointer.
            if matches!(conv, Conv::Str) {
                return format!(
                    "        when :{name} then raise ArgumentError, \"Cannot set :string fields\"\n"
                );
            }
            // Ruby refuses a whole-array assignment too -- the proxy's own
            // `[]=` is how an inline array is written.
            if matches!(conv, Conv::Array(..) | Conv::NestedArray(..)) {
                return format!(
                    "        when :{name} then raise NotImplementedError, \"cannot set array field\"\n"
                );
            }
            // A nested struct write COPIES the value's bytes over the field
            // (oracle-verified memcpy semantics).
            if let Conv::Struct(_, size) = conv {
                return format!(
                    "        when :{name} then @__ffi_ptr.put_bytes({off}, \
                     __ffi_value.to_ptr.get_bytes(0, {size}))\n"
                );
            }
            // A callback write stores a code pointer: a pointer/Function
            // as-is, `nil` as NULL, and any other callable marshaled into an
            // `FFI::Function` -- kept in an ivar so the closure stays alive
            // as long as the struct.
            if let Conv::Callback(args, ret) = conv {
                return format!(
                    "        when :{name} then begin\n           __zeo_v = __ffi_value\n           \
                     unless __zeo_v.nil? || __zeo_v.is_a?(::FFI::Pointer)\n             \
                     __zeo_v = ::FFI::Function.new({ret}, [{args}], __zeo_v)\n             \
                     (@__zeo_cb_keep ||= {{}})[:{name}] = __zeo_v\n           end\n           \
                     @__ffi_ptr.put_pointer({off}, __zeo_v)\n         end\n"
                );
            }
            let value = match conv {
                Conv::Plain => "__ffi_value".to_string(),
                Conv::Bool => "(__ffi_value ? 1 : 0)".to_string(),
                Conv::Str
                | Conv::Array(..)
                | Conv::NestedArray(..)
                | Conv::Struct(..)
                | Conv::Callback(..) => unreachable!("returned above"),
                Conv::Enum(m) => {
                    let table = m
                        .iter()
                        .map(|(n, v)| format!("{n}: {v}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{{{table}}}.fetch(__ffi_value, __ffi_value)")
                }
                Conv::EnumSlot(slot) => format!("__zeo_ffi_enum_put({slot}, __ffi_value)"),
            };
            format!("        when :{name} then @__ffi_ptr.{putter}({off}, {value})\n")
        })
        .collect();
    let offset_arms: String = placed
        .iter()
        .map(|(name, _, _, off, _)| format!("        when :{name} then {off}\n"))
        .collect();
    let members: String = placed
        .iter()
        .map(|(name, ..)| format!(":{name}"))
        .collect::<Vec<_>>()
        .join(", ");
    let offsets: String = placed
        .iter()
        .map(|(name, _, _, off, _)| format!("[:{name}, {off}]"))
        .collect::<Vec<_>>()
        .join(", ");
    let descriptors: String = layout
        .fields
        .iter()
        .zip(&placed)
        .map(|((name, ty, off), _)| ffi_field_descriptor(name, ty, *off))
        .collect::<PResult<Vec<_>>>()?
        .join(",\n    ");

    let inline_array_classes = if with_inline_array_classes {
        FFI_INLINE_ARRAY_CLASSES
    } else {
        ""
    };
    Ok(format!(
        r#"{inline_array_classes}
def initialize(__ffi_ptr = nil)
  @__ffi_ptr = __ffi_ptr || FFI::MemoryPointer.new({total})
end
def [](__ffi_field)
  case __ffi_field
{read_arms}      else raise ArgumentError, "no such struct field #{{__ffi_field.inspect}}"
  end
end
def []=(__ffi_field, __ffi_value)
  case __ffi_field
{write_arms}      else raise ArgumentError, "no such struct field #{{__ffi_field.inspect}}"
  end
  __ffi_value
end
def pointer
  @__ffi_ptr
end
def to_ptr
  @__ffi_ptr
end
def self.size
  {total}
end
def self.alignment
  {alignment}
end
def self.offset_of(__ffi_field)
  case __ffi_field
{offset_arms}      else raise ArgumentError, "no such struct field #{{__ffi_field.inspect}}"
  end
end
def self.members
  [{members}]
end
def self.offsets
  [{offsets}]
end
def offsets
  self.class.offsets
end
def self.layout(*__ffi_args)
  unless __ffi_args.empty?
    raise ::NotImplementedError,
          "a `layout` this class body did not declare isn't supported yet (zeo limitation)"
  end
  @__zeo_layout ||= ::FFI::StructLayout.new([
    {descriptors}
  ], {total}, {alignment})
end
def layout
  self.class.layout
end
"#
    ))
}

/// The library name for `#[link(name = ..)]` from a `ffi_lib` ARGUMENT LIST.
///
/// `ffi_lib` takes CANDIDATES -- several arguments, or one array of them -- and
/// ruby loads the first that dlopens. zeo has to name one library at compile
/// time, so it takes the first candidate it can decide. That is the bare name
/// gems write first (`ffi_lib ["sodium", "libsodium.so.18", "libsodium.so.23"]`
/// -- rbnacl), the later entries being versioned sonames of the same library.
///
/// A candidate it cannot decide is SKIPPED rather than refused: libusb leads
/// with two locals holding bundled paths and then names the system library.
/// A list where nothing at all is decidable answers `None`, and the caller
/// DEFERS the whole statement to runtime evaluation (`FfiLib::Deferred`) --
/// and if a folded name has no library to link against, the build says so,
/// loudly.
fn ffi_lib_name(args: &[Node<'_>], hir: &Hir) -> Option<crate::hir::FfiLib> {
    // Every candidate this can NAME, in the gem's try-in-order semantics. An
    // unfoldable candidate (a local, an ENV read, a helper call) is SKIPPED,
    // exactly as before: only an all-unfoldable list errors.
    let mut folded: Vec<String> = Vec::new();
    for a in args {
        match a.as_array_node() {
            Some(arr) => {
                for e in arr.elements().iter() {
                    if let Some(s) = fold_lib_string(&e, hir) {
                        folded.push(s);
                    }
                }
            }
            None => {
                if let Some(s) = fold_lib_string(a, hir) {
                    folded.push(s);
                }
            }
        }
    }
    let first = folded.first()?;
    // A plain name links at BUILD time -- the zero-overhead tier, and what
    // every `ffi_lib "m"` always got. A PATH only a running process can
    // resolve (a bundled `.so` beside the gem's own files) goes to the
    // runtime dlopen tier, which is when and where CRuby's ffi gem opens
    // every library.
    if !first.contains('/') {
        return Some(crate::hir::FfiLib::Static(strip_lib_name(first)));
    }
    Some(crate::hir::FfiLib::Runtime(folded))
}

/// A candidate's compile-time STRING value, or `None` when it is only
/// decidable at run time (a local, an ENV read, a helper call).
///
/// NOT special-cased: `ffi_lib FFI.library_name("vips", 42)` (ruby-vips).
/// `library_name` is not an `ffi` API -- ruby-vips reopens `module FFI` and
/// defines it -- so folding it by name would hard-code one gem's helper into
/// the compiler and miscompile the next gem to pick the same name.
fn fold_lib_string(node: &Node<'_>, hir: &Hir) -> Option<String> {
    if let Some(s) = node.as_string_node() {
        return Some(String::from_utf8_lossy(s.unescaped()).into_owned());
    }
    // `ffi_lib :kernel32, :user32` -- windows gems name their DLLs as symbols.
    if let Some(s) = node.as_symbol_node() {
        return Some(String::from_utf8_lossy(s.unescaped()).into_owned());
    }
    // `"#{__dir__}/../ext/libfoo.so"` -- parts fold independently.
    if let Some(interp) = node.as_interpolated_string_node() {
        let mut out = String::new();
        for part in interp.parts().iter() {
            if let Some(s) = part.as_string_node() {
                out.push_str(&String::from_utf8_lossy(s.unescaped()));
            } else if let Some(embedded) = part.as_embedded_statements_node() {
                let stmts: Vec<Node<'_>> = embedded
                    .statements()
                    .map(|s| s.body().iter().collect())
                    .unwrap_or_default();
                let [only] = stmts.as_slice() else {
                    return None;
                };
                out.push_str(&fold_lib_string(only, hir)?);
            } else {
                return None;
            }
        }
        return Some(out);
    }
    if let Some(call) = node.as_call_node() {
        let name = call.name().as_slice();
        let args: Vec<Node<'_>> = call
            .arguments()
            .map(|a| a.arguments().iter().collect())
            .unwrap_or_default();
        // The requiring file's directory -- how a gem roots its bundled
        // native half.
        if name == b"__dir__" && call.receiver().is_none() && args.is_empty() {
            return Some(hir.lowering_dir.as_ref()?.display().to_string());
        }
        if name == b"+"
            && let Some(recv) = call.receiver()
            && let [rhs] = args.as_slice()
        {
            return Some(format!(
                "{}{}",
                fold_lib_string(&recv, hir)?,
                fold_lib_string(rhs, hir)?
            ));
        }
        let on_file = call
            .receiver()
            .and_then(|r| const_path_string(&r))
            .is_some_and(|p| p.trim_start_matches("::") == "File");
        if on_file {
            match (name, args.as_slice()) {
                (b"join", parts) if !parts.is_empty() => {
                    let folded: Option<Vec<String>> =
                        parts.iter().map(|p| fold_lib_string(p, hir)).collect();
                    return Some(folded?.join("/"));
                }
                (b"dirname", [p]) => {
                    let s = fold_lib_string(p, hir)?;
                    let parent = std::path::Path::new(&s).parent()?;
                    return Some(parent.display().to_string());
                }
                (b"expand_path", [p]) => {
                    return Some(lexical_join(
                        &hir.lowering_dir.clone()?,
                        &fold_lib_string(p, hir)?,
                    ));
                }
                (b"expand_path", [p, base]) => {
                    let base = fold_lib_string(base, hir)?;
                    return Some(lexical_join(
                        std::path::Path::new(&base),
                        &fold_lib_string(p, hir)?,
                    ));
                }
                _ => return None,
            }
        }
        return None;
    }
    // `FFI::Library::LIBC` names the platform C library (`c`, which resolves
    // to libSystem on macOS). `FFI::Platform::LIBC` is the same constant by
    // its other path, and either may be written `::`-anchored.
    let path = const_path_string(node)?;
    match path.trim_start_matches("::") {
        "FFI::Library::LIBC" | "FFI::Platform::LIBC" => Some("c".to_string()),
        _ => None,
    }
}

/// `base` joined with `rel`, `.`/`..` collapsed LEXICALLY -- the same rule as
/// `File.expand_path`, which never consults the filesystem.
fn lexical_join(base: &std::path::Path, rel: &str) -> String {
    use std::path::Component;
    let mut out = std::path::PathBuf::new();
    for component in base.join(rel).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out.display().to_string()
}

/// Whether any `ffi_lib` argument names the process itself.
fn names_current_process(args: &[Node<'_>]) -> bool {
    let is_marker = |n: &Node<'_>| {
        const_path_string(n).is_some_and(|p| {
            matches!(
                p.trim_start_matches("::"),
                "FFI::CURRENT_PROCESS" | "FFI::USE_THIS_PROCESS_AS_LIBRARY"
            )
        })
    };
    args.iter().any(|a| match a.as_array_node() {
        Some(arr) => arr.elements().iter().any(|e| is_marker(&e)),
        None => is_marker(a),
    })
}

/// `libm.so.6` / `libssl.dylib` / `m` -> the bare rustc link name (`m`/`ssl`).
fn strip_lib_name(raw: &str) -> String {
    let base = raw.rsplit('/').next().unwrap_or(raw);
    let base = base.split(['.']).next().unwrap_or(base);
    base.strip_prefix("lib").unwrap_or(base).to_string()
}

/// `attach_function :abs, [:int], :int` (plain) or `attach_function :my_len,
/// :strlen, [:string], :ulong` (the 4-arg rename form) -> a synthesized class
/// method whose body is a `HirNode::Ffi` over the C symbol.
fn lower_attach_function(
    result: &ParseResult,
    hir: &mut Hir,
    args: &[Node<'_>],
    lib: crate::hir::FfiLib,
    aliases: &crate::compiler::FMap<String, crate::hir::FfiType>,
    class_body: &[Node<'_>],
) -> PResult<NodeId> {
    let _ = result;
    // `attach_function(name, func = name, args, returns, options = {})`. The
    // options are ruby's own trailing hash, so prism hands them over as one
    // more element of the argument list -- 3/4 arguments plus an optional one.
    let (positional, options) = match args.last().filter(|a| is_options_hash(a)) {
        Some(opts) => (&args[..args.len() - 1], Some(opts)),
        None => (args, None),
    };
    let (ruby_name, c_symbol, types_node, ret_node) = match positional.len() {
        3 => {
            let name = ffi_c_name(&positional[0], class_body)?;
            (name.clone(), name, &positional[1], &positional[2])
        }
        4 => (
            ffi_symbol_str(&positional[0])?,
            ffi_c_name(&positional[1], class_body)?,
            &positional[2],
            &positional[3],
        ),
        n => {
            return Err(format!(
                "attach_function expects 3 or 4 arguments (name, [args], ret), got {n}"
            )
            .into());
        }
    };
    let (arg_types, variadic) = ffi_arg_types(types_node, aliases, class_body)?;
    let ret = ffi_type_node(ret_node, aliases, TypePos::Signature)?;
    let blocking = match options {
        Some(opts) => attach_function_options(opts, class_body)?,
        None => false,
    };
    // A struct REFERENCE degrades here, at the declaration: an argument is
    // the plain pointer (`to_pointer` already auto-converts a struct via
    // `to_ptr`, the gem's own rule), and a RETURN is a plain `FFI::Pointer`
    // -- oracle-verified, the gem does NOT auto-wrap a returned pointer in
    // the class. Codegen never sees `StructRef`.
    let arg_types: Vec<crate::hir::FfiType> = arg_types
        .into_iter()
        .map(|t| match t {
            crate::hir::FfiType::StructRef(_) => crate::hir::FfiType::Pointer,
            t => t,
        })
        .collect();
    let ret = match ret {
        crate::hir::FfiType::StructRef(_) => crate::hir::FfiType::Pointer,
        ret => ret,
    };
    // By-value structs ride the fixed `extern "C"` tier, where rustc owns the
    // ABI -- as arguments AND as a return (the wrapper below views the
    // returned bytes through the struct's own class). The positions the
    // other tiers cannot express are clean rejections at the declaration --
    // never a wrong call.
    let ret_struct_class = match &ret {
        crate::hir::FfiType::Struct(l) if !l.class_path.is_empty() => Some(l.class_path.clone()),
        crate::hir::FfiType::Struct(_) => {
            return Err(
                "returning a struct BY VALUE needs a NAMED `FFI::Struct` class to wrap it in \
                 (zeo limitation) -- return `.by_ref` (a pointer) and wrap it"
                    .to_string()
                    .into(),
            );
        }
        _ => None,
    };
    // `:strptr` reads a RETURNED `char *` twice (string + pointer); the gem
    // has no argument meaning for it either.
    if arg_types
        .iter()
        .any(|t| matches!(t, crate::hir::FfiType::StrPtr))
    {
        return Err(
            "`:strptr` is only usable as an `attach_function` return type"
                .to_string()
                .into(),
        );
    }
    // A UNION by value has no honest aggregate descriptor on either tier
    // (the mirror's field asserts would overlap; libffi has no union type),
    // and the extern tier's mirror asserts would reject it at build time
    // anyway -- say it here, at the declaration.
    let union_by_value = std::iter::once(&ret)
        .chain(arg_types.iter())
        .any(|t| matches!(t, crate::hir::FfiType::Struct(l) if l.union));
    if union_by_value {
        return Err(
            "a union passed or returned BY VALUE isn't supported yet (zeo limitation) -- \
             use `.by_ref`"
                .to_string()
                .into(),
        );
    }
    let by_value = ret_struct_class.is_some()
        || arg_types
            .iter()
            .any(|t| matches!(t, crate::hir::FfiType::Struct(_)));
    if by_value && variadic {
        return Err(
            "a variadic `attach_function` can't pass or return a struct BY VALUE (zeo \
             limitation) -- use `.by_ref`"
                .to_string()
                .into(),
        );
    }
    // `blocking: true` beside a callback argument is fine in every mode: the
    // default parallel mode has no GVL to release, and under `ZEO_GVL=1` the
    // callback trampoline re-acquires before entering ruby (see
    // `zeo_rt::ffi::invoke_callback`). rdkafka's poll loop is the shape.

    // The wrapper's params: one required positional per FIXED C argument, named
    // so a `LocalRead` in the `Ffi` body reaches it; a variadic function also
    // gets a `*__ffi_rest` that collects the trailing (type, value) pairs.
    let param_names: Vec<String> = (0..arg_types.len())
        .map(|i| format!("__ffi_a{i}"))
        .collect();
    let call_args: Vec<(NodeId, crate::hir::FfiType)> = param_names
        .iter()
        .zip(arg_types)
        .map(|(name, ty)| (hir.push(HirNode::LocalRead(name.clone())), ty))
        .collect();
    let variadic_read = variadic.then(|| hir.push(HirNode::LocalRead("__ffi_rest".to_string())));
    let ffi_node = hir.push(HirNode::Ffi(crate::hir::FfiCall {
        symbol: c_symbol,
        lib,
        args: call_args,
        ret,
        variadic: variadic_read,
        blocking,
    }));
    // A by-value struct return comes out of the call as a fresh ruby-owned
    // `MemoryPointer` (see codegen's `emit_ffi_call`); the struct class's own
    // `new(pointer)` then views it -- the same wrap the accessor synthesis
    // uses for a nested field, resolved lexically from the attach site.
    let body = match ret_struct_class {
        Some(class_name) => vec![hir.push(HirNode::New {
            class_name,
            args: vec![ffi_node],
            kwargs: Vec::new(),
            block: None,
        })],
        None => vec![ffi_node],
    };
    let params = Params {
        required: param_names,
        rest: variadic.then(|| Some("__ffi_rest".to_string())),
        ..Default::default()
    };
    Ok(hir.push(HirNode::DefMethod {
        name: ruby_name,
        params,
        body,
        is_class_method: true,
        visibility: Visibility::Public,
        is_def: true,
    }))
}

/// Whether a trailing `attach_function` argument is its options hash rather
/// than a type. Only the hash forms qualify, so a 5th POSITIONAL argument is
/// still the arity error it was.
fn is_options_hash(node: &Node<'_>) -> bool {
    node.as_keyword_hash_node().is_some() || node.as_hash_node().is_some()
}

/// `attach_function`'s options hash -> whether the call releases the GVL.
///
/// `blocking: true` is the one option with meaning on a target zeo builds for.
/// `convention:` decides between cdecl and stdcall, which is a 32-bit Windows
/// distinction -- the real gem ignores it everywhere else, and so does this.
/// `enums:` and `type_map:` change how VALUES marshal, so an unrecognized or
/// non-literal option stays a clean rejection rather than a silent drop.
fn attach_function_options<'a>(node: &Node<'a>, class_body: &[Node<'a>]) -> PResult<bool> {
    let elements: Vec<Node<'a>> = match node.as_keyword_hash_node() {
        Some(k) => k.elements().iter().collect(),
        None => node
            .as_hash_node()
            .map(|h| h.elements().iter().collect())
            .unwrap_or_default(),
    };
    // `**opts`, where the class body set `opts = { blocking: true }` above --
    // cztop, mosq, jansson and czmq-ffi-gen all hoist the one option they
    // share into a local and splat it into every declaration. Resolved
    // through the same body-local replay a computed C name uses.
    let mut pairs: Vec<Node<'a>> = Vec::new();
    for element in elements {
        let Some(splat) = element.as_assoc_splat_node() else {
            pairs.push(element);
            continue;
        };
        let hash = splat
            .value()
            .as_ref()
            .and_then(local_read_name)
            .and_then(|n| body_local_value(&n, class_body, node.location().end_offset()))
            .and_then(|v| v.as_hash_node().map(|h| h.elements().iter().collect()))
            .ok_or_else(|| {
                "attach_function's options must be literal `key: value` pairs".to_string()
            })?;
        pairs.extend::<Vec<Node<'a>>>(hash);
    }
    let mut blocking = false;
    for pair in pairs {
        let assoc = pair.as_assoc_node().ok_or_else(|| {
            "attach_function's options must be literal `key: value` pairs".to_string()
        })?;
        let key = assoc
            .key()
            .as_symbol_node()
            .map(|s| String::from_utf8_lossy(s.unescaped()).into_owned())
            .ok_or_else(|| {
                "attach_function's options must be literal `key: value` pairs".to_string()
            })?;
        match key.as_str() {
            "blocking" => {
                let v = assoc.value();
                blocking = match () {
                    _ if v.as_true_node().is_some() => true,
                    _ if v.as_false_node().is_some() || v.as_nil_node().is_some() => false,
                    _ => {
                        return Err("attach_function's `blocking:` expects `true` or `false`"
                            .to_string()
                            .into());
                    }
                };
            }
            "convention" => {}
            other => {
                return Err(format!(
                    "attach_function option `{other}:` isn't supported yet (zeo limitation) -- \
                     `blocking:` and `convention:` are"
                )
                .into());
            }
        }
    }
    Ok(blocking)
}

/// A Symbol node's name (`:abs` -> `"abs"`). FFI names/types are always literal
/// symbols; anything else is a clean rejection.
/// The node an FFI declaration position really reads, past the spellings that
/// only WRAP a value. ffi-tk writes `attach_function :Tk_GetColor, [:pointer,
/// :pointer, name = :string], :pointer` -- an assignment mid-list, whose value
/// is the type and whose local is used further down the file. Parentheses wrap
/// the same way (`typedef (COND ? :long_long : :long), :json_int`).
fn ffi_written_value<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    let unwrap_once = |n: &Node<'a>| -> Option<Node<'a>> {
        if let Some(w) = n.as_local_variable_write_node() {
            return Some(w.value());
        }
        let body: Vec<Node<'a>> = n
            .as_parentheses_node()?
            .body()?
            .as_statements_node()?
            .body()
            .iter()
            .collect();
        <[Node<'a>; 1]>::try_from(body).ok().map(|[only]| only)
    };
    let mut cur = unwrap_once(node)?;
    while let Some(inner) = unwrap_once(&cur) {
        cur = inner;
    }
    Some(cur)
}

fn ffi_symbol_str(node: &Node<'_>) -> PResult<String> {
    let written = ffi_written_value(node);
    let node = written.as_ref().unwrap_or(node);
    // A string literal names the same thing: the ffi gem calls `.to_sym` on
    // its name arguments, and `attach_function 'rados_seek', ...` is common.
    node.as_symbol_node()
        .map(|s| String::from_utf8_lossy(s.unescaped()).into_owned())
        .or_else(|| {
            node.as_string_node()
                .map(|s| String::from_utf8_lossy(s.unescaped()).into_owned())
        })
        .ok_or_else(|| "expected a literal symbol or string in an FFI declaration".into())
}

/// One FFI type as WRITTEN in a declaration. Three spellings reach a type
/// position:
///
///  - a literal symbol -- `:int`, `:pointer`, or a declared alias;
///  - `Status.by_ref` / `Status.ptr` -- a POINTER to a struct, which is what
///    the C prototype takes and what libffi is handed either way. The struct's
///    own layout never enters the call, so this needs nothing from it. (`.by_value`
///    passes the struct itself and does need the layout, so it stays rejected);
///  - a constant naming a declared `enum`/`typedef`/`callback`, which is how
///    the anonymous `Tag = enum(...)` form is referred to afterwards. Only the
///    LEAF name is looked up: the table is keyed by the name as declared, and
///    these are always written inside the library module that declared them.
/// A callback ARGUMENT naming a struct class degrades to a pointer, which is
/// what the Proc really receives.
///
/// This used to be a clean rejection, on the belief that ruby-ffi hands the
/// Proc a Struct instance there. It does not. `StructByReference#from_native`
/// would build one, but the callback path never routes an argument through the
/// Ruby data converter -- oracle-verified with `qsort` over a `[Pair, Pair]`
/// comparator, whose block receives an `FFI::Pointer`. The old rejection even
/// told the author to "take `:pointer` and wrap it yourself", which is exactly
/// what the gem does for them.
///
/// A RETURN in that position stays rejected: nothing here measured it, and a
/// silently wrong conversion is worse than a refusal.
fn degrade_callback_struct_refs(
    arg_types: &mut [crate::hir::FfiType],
    ret_ty: &crate::hir::FfiType,
) -> PResult<()> {
    if matches!(ret_ty, crate::hir::FfiType::StructRef(_)) {
        return Err(
            "a callback RETURNING a struct class isn't supported yet (zeo limitation) \
             -- return `:pointer` and wrap it in the struct class yourself"
                .to_string()
                .into(),
        );
    }
    for t in arg_types {
        if matches!(t, crate::hir::FfiType::StructRef(_)) {
            *t = crate::hir::FfiType::Pointer;
        }
    }
    Ok(())
}

/// Where a type expression is WRITTEN, which decides what a bare struct name
/// means: ruby-ffi's `find_type` wraps a struct class as `StructByReference`
/// (a pointer) in a signature/typedef position, while a layout FIELD naming a
/// struct class embeds it inline, by value.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TypePos {
    Signature,
    Field,
}

fn ffi_type_node(
    node: &Node<'_>,
    aliases: &crate::compiler::FMap<String, crate::hir::FfiType>,
    pos: TypePos,
) -> PResult<crate::hir::FfiType> {
    let written = ffi_written_value(node);
    let node = written.as_ref().unwrap_or(node);
    // An INLINE `callback([...], ret)` in a type position -- the anonymous
    // twin of the named `callback :tag, [...], ret` declaration, carrying the
    // same signature without registering a name.
    if let Some(call) = node.as_call_node()
        && call.receiver().is_none()
        && call.name().as_slice() == b"callback"
        && let Some(arguments) = call.arguments()
    {
        let args: Vec<Node<'_>> = arguments.arguments().iter().collect();
        let [params, ret] = args.as_slice() else {
            return Err(
                "callback expects `[arg_types], return_type` in a type position"
                    .to_string()
                    .into(),
            );
        };
        // No class body to replay here: `ffi_type_node` is reached from every
        // type position, and threading one through all of them for the
        // anonymous-callback arm alone is not worth it. The literal, `%i[..]`
        // and `[..] * n` spellings still resolve; only a body-local list
        // written inside an INLINE callback would not.
        let mut arg_types = ffi_type_array(params, aliases, &[])?;
        let ret_ty = ffi_type_node(ret, aliases, TypePos::Signature)?;
        degrade_callback_struct_refs(&mut arg_types, &ret_ty)?;
        return Ok(crate::hir::FfiType::Callback(arg_types, Box::new(ret_ty)));
    }
    if let Some(call) = node.as_call_node()
        && call.receiver().is_some()
    {
        // The ffi gem's DIRECTION annotations -- `Struct.ptr(:in)`,
        // `Status.in`/`.out`/`.inout`, `Foo.ptr.out` -- all pass a POINTER;
        // the direction only tunes the gem's own marshaling copies, never
        // the ABI, so every spelling is the plain pointer type here.
        let direction_arg = call.arguments().is_none_or(|a| {
            let args: Vec<Node<'_>> = a.arguments().iter().collect();
            matches!(args.as_slice(), [d] if d.as_symbol_node().is_some())
        });
        match call.name().as_slice() {
            b"by_ref" | b"ptr" if direction_arg => return Ok(crate::hir::FfiType::Pointer),
            b"in" | b"out" | b"inout" if call.arguments().is_none() => {
                return Ok(crate::hir::FfiType::Pointer);
            }
            _ => {}
        }
    }
    if let Some(call) = node.as_call_node()
        && let Some(recv) = call.receiver()
        && call.arguments().is_none()
    {
        match call.name().as_slice() {
            b"by_value" | b"val" => {
                // The receiver must be a struct whose `layout` already
                // lowered -- the by-value ABI needs the full field list.
                let path = const_path_string(&recv).unwrap_or_default();
                let leaf = path.rsplit("::").next().unwrap_or(&path);
                return match aliases.get(leaf) {
                    Some(t @ crate::hir::FfiType::Struct(_)) => Ok(t.clone()),
                    _ => Err(format!(
                        "`{path}.by_value` needs an `FFI::Struct` whose `layout` lowered earlier \
                         in this program"
                    )
                    .into()),
                };
            }
            _ => {}
        }
    }
    if let Some(path) = const_path_string(node) {
        let leaf = path.rsplit("::").next().unwrap_or(&path);
        return match aliases.get(leaf) {
            // A bare struct name in a signature is ruby-ffi's
            // `StructByReference` -- a POINTER, never the by-value ABI
            // (`.by_value` spells that). A LAYOUT field keeps the inline
            // by-value struct.
            Some(crate::hir::FfiType::Struct(l)) if pos == TypePos::Signature => {
                Ok(crate::hir::FfiType::StructRef(l.class_path.clone()))
            }
            Some(t) => Ok(t.clone()),
            None => Err(format!(
                "`{path}` isn't a declared FFI type (expected an `enum`/`typedef`/`callback` \
                 declared earlier in this library)"
            )
            .into()),
        };
    }
    ffi_type_of(&ffi_symbol_str(node)?, aliases)
}

/// The argument-type list of an `attach_function`, splitting a trailing
/// `:varargs` marker: `[:string, :varargs]` -> `([Str], true)`. `:varargs` is
/// only legal as the final element (a variadic function's fixed prototype ends
/// before it); anywhere else is a clean rejection.
fn ffi_arg_types<'a>(
    node: &Node<'a>,
    aliases: &crate::compiler::FMap<String, crate::hir::FfiType>,
    class_body: &[Node<'a>],
) -> PResult<(Vec<crate::hir::FfiType>, bool)> {
    let (elems, repeat) = type_list(node, class_body)
        .ok_or_else(|| "attach_function's argument list must be a literal array".to_string())?;
    let mut types = Vec::new();
    let mut variadic = false;
    for (i, el) in elems.iter().enumerate() {
        // `:varargs` is a marker rather than a type, so it is read off the
        // literal symbol before anything else; every other element is an
        // ordinary type position and may be written any of the three ways.
        if el.as_symbol_node().is_some() && ffi_symbol_str(el)? == "varargs" {
            if i != elems.len() - 1 {
                return Err("`:varargs` must be the last FFI argument type"
                    .to_string()
                    .into());
            }
            variadic = true;
        } else {
            types.push(ffi_type_node(el, aliases, TypePos::Signature)?);
        }
    }
    Ok((repeated(types, repeat), variadic))
}

/// `list` laid end to end `n` times. `[T]::repeat` wants `Copy`, which an
/// `FfiType` carrying a whole struct layout is not.
fn repeated(list: Vec<crate::hir::FfiType>, n: usize) -> Vec<crate::hir::FfiType> {
    match n {
        1 => list,
        _ => (0..n).flat_map(|_| list.iter().cloned()).collect(),
    }
}

/// The ELEMENTS of a declared type list, and how many times the list repeats.
///
/// Three spellings beyond the plain literal array, all of them a gem writing
/// out a prototype it did not want to spell twice: `[:pointer] * 13` (rbmetis'
/// METIS bindings, csspool's croco callbacks), a body-local the class set to
/// one (`params = %i[string int int]`, ires), and a `%i[...]` word list. The
/// repeat rides back as a COUNT rather than repeated nodes: a prism `Node` is
/// not `Clone`, and repeating the resolved types is the same answer.
fn type_list<'a>(node: &Node<'a>, class_body: &[Node<'a>]) -> Option<(Vec<Node<'a>>, usize)> {
    if let Some(syms) = word_list_to_syms(node) {
        return Some((syms, 1));
    }
    if let Some(array) = node.as_array_node() {
        return Some((array.elements().iter().collect(), 1));
    }
    if let Some(name) = local_read_name(node)
        && let Some(elems) = local_array_elements(&name, class_body, node.location().start_offset())
    {
        return Some((elems, 1));
    }
    // `[...] * n`
    let call = node.as_call_node()?;
    if call.name().as_slice() != b"*" {
        return None;
    }
    let args: Vec<Node<'a>> = call.arguments()?.arguments().iter().collect();
    let [count] = args.as_slice() else {
        return None;
    };
    let value = count.as_integer_node()?.value();
    let (negative, digits) = value.to_u32_digits();
    let count = usize::try_from(assemble_i64(negative, digits)?).ok()?;
    let (elems, _) = type_list(&call.receiver()?, class_body)?;
    Some((elems, count))
}

/// `[:int, :string]` -> `[Int(32), Str]`. A declared type list in a position
/// with no `:varargs` marker (a `callback` signature).
fn ffi_type_array<'a>(
    node: &Node<'a>,
    aliases: &crate::compiler::FMap<String, crate::hir::FfiType>,
    class_body: &[Node<'a>],
) -> PResult<Vec<crate::hir::FfiType>> {
    let (elems, repeat) = type_list(node, class_body)
        .ok_or_else(|| "attach_function's argument list must be a literal array".to_string())?;
    let types: Vec<crate::hir::FfiType> = elems
        .iter()
        .map(|el| ffi_type_node(el, aliases, TypePos::Signature))
        .collect::<PResult<_>>()?;
    Ok(repeated(types, repeat))
}

/// Map a real `ffi`-gem type keyword to our `FfiType` -- the scalar keyword
/// table lives in `zeo_abi::ffi::CScalar`, shared with the runtime's own
/// varargs/`FFI::Type` resolution so the two sides cannot drift. A keyword
/// misses to the declared aliases, then to the portable C/Win32 typedef
/// table (also `CScalar`'s; the doc comments there say why several POSIX
/// names are deliberately absent). Unknown -> a clean, greppable error.
pub(crate) fn ffi_type_of(
    sym: &str,
    aliases: &crate::compiler::FMap<String, crate::hir::FfiType>,
) -> PResult<crate::hir::FfiType> {
    if let Some(s) = zeo_abi::ffi::CScalar::from_keyword(sym) {
        return Ok(s.into());
    }
    if sym == "strptr" {
        return Ok(crate::hir::FfiType::StrPtr);
    }
    if let Some(t) = aliases.get(sym) {
        return Ok(t.clone());
    }
    if let Some(s) = zeo_abi::ffi::CScalar::from_c_typedef(sym) {
        return Ok(s.into());
    }
    // The POSIX integer typedefs whose width GENUINELY differs between the
    // targets zeo builds for -- `CScalar::from_c_typedef`'s documented
    // rejections. Legal in argument/return position, where the generated
    // code spells the target's own `libc::<name>`; a struct layout still
    // rejects them (see `ffi_field_accessor`).
    let bare = sym.strip_prefix("__").unwrap_or(sym);
    if platform_scalar_of(bare).is_some() {
        return Ok(crate::hir::FfiType::PlatformScalar(bare.to_string()));
    }
    Err(format!(
        "unsupported FFI type `:{sym}` (expected a scalar keyword, `:pointer`, `:string`, a C typedef whose width is the same on every target zeo builds for, or a declared `typedef`/`enum`/`callback` name)"
    ).into())
}
