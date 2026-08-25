//! Pure AST-shape matching for the FFI directive family: the pre-lower
//! prescan, `extend FFI::Library` recognition, and the literal / constant /
//! local-array replay helpers the sibling `ffi` modules share. No HIR
//! emission happens here.

use super::directive::{as_ffi_layout, hash_pairs, parse_enum_members};
use super::synth::ffi_struct_layout;
use super::types::{TypePos, ffi_symbol_str, ffi_type_node, ffi_type_of};
use crate::hir::Hir;
use crate::lower::PResult;
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
    hir.ffi.ffi_struct_layouts.insert(leaf.to_string(), layout);
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
pub(super) fn splat_local_elements<'a>(
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
pub(super) fn local_array_elements<'a>(
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
pub(super) fn word_list_to_syms<'a>(node: &Node<'a>) -> Option<Vec<Node<'a>>> {
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
pub(super) fn ffi_c_name(node: &Node<'_>, class_body: &[Node<'_>]) -> PResult<String> {
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
        let taken = match crate::lower::defs::static_guard(&if_node.predicate())? {
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
pub(super) fn body_local_value<'a>(
    name: &str,
    body: &[Node<'a>],
    before: usize,
) -> Option<Node<'a>> {
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
            && let Some(cond) = crate::lower::defs::static_guard(&if_node.predicate())
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
            && let Some(cond) = crate::lower::defs::static_guard(&unless_node.predicate())
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

/// Receiver-position calls that change the object in place -- the poison
/// vocabulary both the local and the constant replays read. Any `!` method
/// counts too; everything else leaves the value alone.
const IN_PLACE_METHODS: &[&str] = &[
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

/// `expr.freeze` -> `expr`. A frozen value is the same value, and a shared
/// prototype is conventionally written frozen.
pub(super) fn unwrap_freeze<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    let call = node.as_call_node()?;
    if call.name().as_slice() != b"freeze" || call.arguments().is_some() || call.block().is_some() {
        return None;
    }
    call.receiver()
}

/// `(expr)` -> `expr`.
pub(super) fn unwrap_parens<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    let body = node.as_parentheses_node()?.body()?;
    let stmts = body.as_statements_node()?;
    let mut it = stmts.body().iter();
    let one = it.next()?;
    it.next().is_none().then_some(one)
}

/// The name a plain CONSTANT read spells, or `None` for any other node. Only
/// the bare form: a `Foo::BAR` names a scope this stage does not resolve.
pub(super) fn const_read_name(node: &Node<'_>) -> Option<String> {
    let read = node.as_constant_read_node()?;
    Some(String::from_utf8_lossy(read.name().as_slice()).into_owned())
}

/// The VALUE a class body's `NAME = ...` bound, read at a declaration BEFORE
/// any later statement could have changed it -- `local_array_elements`'
/// constant twin, answering the node rather than elements so the caller can
/// read it with the same rules every other type-list position gets.
///
/// A constant is written once by convention, so there is no append idiom to
/// replay: a second write, or any in-place mutation, poisons the read rather
/// than answering with a value that may already be stale.
pub(super) fn const_value_node<'a>(
    name: &str,
    body: &[Node<'a>],
    before: usize,
) -> Option<Node<'a>> {
    let mut found: Option<Node<'a>> = None;
    for stmt in body {
        if stmt.location().start_offset() >= before {
            continue;
        }
        if let Some(write) = stmt.as_constant_write_node()
            && String::from_utf8_lossy(write.name().as_slice()) == name
        {
            if found.is_some() {
                return None;
            }
            found = Some(write.value());
            continue;
        }
        if const_mutated(name, stmt) {
            return None;
        }
    }
    found
}

/// Whether a statement mutates the constant in place (`TYPES << :int`) --
/// `local_mutated`'s constant twin, over the same in-place vocabulary.
fn const_mutated(name: &str, stmt: &Node<'_>) -> bool {
    struct Search<'n> {
        name: &'n str,
        found: bool,
    }
    impl<'pr> ruby_prism::Visit<'pr> for Search<'_> {
        fn visit_call_node(&mut self, node: &ruby_prism::CallNode<'pr>) {
            if let Some(recv) = node.receiver()
                && const_read_name(&recv).as_deref() == Some(self.name)
            {
                let method = String::from_utf8_lossy(node.name().as_slice()).into_owned();
                self.found |= method.ends_with('!') || IN_PLACE_METHODS.contains(&method.as_str());
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
    let mut search = Search { name, found: false };
    ruby_prism::Visit::visit(&mut search, stmt);
    search.found
}

/// The name a plain local-variable READ spells, or `None` for any other node.
pub(super) fn local_read_name(node: &Node<'_>) -> Option<String> {
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
                self.found |= method.ends_with('!') || IN_PLACE_METHODS.contains(&method.as_str());
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
