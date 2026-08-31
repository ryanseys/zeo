//! A `def` a RUNTIME alias names as its SOURCE is installed POSITIONALLY.
//!
//! `alias_method` copies the METHOD ENTRY that exists when it runs -- CRuby's
//! `rb_alias` -- so an alias taken above a `def` binds whatever the name meant
//! THEN, and a later `def` of the same name does not change it:
//!
//!     class T1; end
//!     T1.class_eval { alias_method :eql?, :== }   # copies BasicObject#==
//!     class T1
//!       def ==(other) = true                      # does NOT reach the alias
//!     end
//!     T1.new.eql?(T1.new)                         # false
//!
//! zeo registers a compiled `def` at program START, so at the alias's line the
//! class already answered `==` with the later body and the alias copied that.
//!
//! The row is CONCEALED until the `def`'s own line instead -- the same
//! machinery a swept unit's methods use ([`crate::clif::classes`]'s conceal
//! rows and `dispatch::concealed`), with a reveal group of its own spliced at
//! the definition's document position rather than at a unit function's head.
//! A concealed name never terminates a lookup, so the walk falls through to
//! the ancestors, which is what ruby does with a method not yet defined.
//!
//! Only a LITERAL source name is followed. A computed one
//! (`alias_method(a, b)`) leaves every `def` eager, which is the behaviour
//! this pass replaces and is written down rather than guessed at.
//!
//! Must run after `redefs::resolve`: both splice into a site's statements and
//! bump the `at` of every later `def`, and the reveal has to land at the
//! position the redefinition splice left.

use crate::compiler::{ClassId, Compiler, DefEvent};
use crate::hir::{ArrayElem, HirNode};

pub fn resolve(compiler: &mut Compiler) {
    let sources = runtime_alias_sources(compiler);
    if sources.is_empty() {
        return;
    }
    // Reveal groups are numbered past the units', because both index one
    // table at run time (`dispatch::concealed`'s `REVEALED`).
    let mut group = compiler.hir.loader.feature_units.len() as u32;

    let mut sites_by_class: crate::compiler::FMap<ClassId, Vec<usize>> = Default::default();
    for (si, site) in compiler.class_body_sites.iter().enumerate() {
        sites_by_class.entry(site.class).or_default().push(si);
    }

    for idx in 0..compiler.classes.len() {
        let cid = ClassId(idx as u32);
        // A class a UNIT wrote already conceals its whole set, and a builtin's
        // rows are native -- neither has a compiled row to hold back.
        if compiler.classes[idx].unit.is_some() || compiler.classes[idx].is_builtin {
            continue;
        }
        let names: Vec<(String, bool)> = compiler.classes[idx]
            .method_history
            .iter()
            .filter(|(n, _, _, _)| sources.contains(n.as_str()))
            .map(|(n, cm, _, _)| (n.clone(), *cm))
            .collect::<crate::compiler::FSet<_>>()
            .into_iter()
            .collect();
        for (name, singleton) in names {
            // Every body of this name, with the document position of each.
            // A body with no site record -- a `def` inside an `if` -- has no
            // position to reveal at, so the name stays eager.
            let mut defs: Vec<(usize, usize, u32)> = Vec::new();
            for &si in sites_by_class.get(&cid).map_or(&[][..], Vec::as_slice) {
                for (di, d) in compiler.class_body_sites[si].defs.iter().enumerate() {
                    if d.event == DefEvent::Added && d.singleton == singleton && d.name == name {
                        defs.push((si, di, d.seq));
                    }
                }
            }
            let bodies = compiler.classes[idx]
                .method_history
                .iter()
                .filter(|(n, cm, _, _)| *cm == singleton && *n == name)
                .count();
            if defs.is_empty() || defs.len() != bodies {
                continue;
            }
            defs.sort_by_key(|&(_, _, seq)| seq);

            // ONE group for the name: the first body's position reveals it,
            // and a later body re-installs through `redefs` as it already did.
            let g = group;
            group += 1;
            compiler.alias_source_reveals.push((cid, name.clone(), singleton, g));

            let (si, di, _) = defs[0];
            let (def_at, def_seq) = {
                let d = &compiler.class_body_sites[si].defs[di];
                (d.at, d.seq)
            };
            let node = compiler.hir.push(HirNode::MethodReveal(g));
            compiler.class_body_sites[si].stmts.insert(def_at, node);
            for d in &mut compiler.class_body_sites[si].defs {
                if d.at > def_at || (d.at == def_at && d.seq >= def_seq) {
                    d.at += 1;
                }
            }
        }
    }
}

/// Every SOURCE name a surviving `alias_method` call names literally.
///
/// The compile-time form (`HirNode::AliasMethod`) is not here: it resolves in
/// `mro::resolve_aliases`, which binds the body the alias's own seq can see.
/// What is left is a real send -- `T1.class_eval { alias_method :a, :b }`,
/// `T1.send(:alias_method, :a, :b)`, `define_method`'s neighbours -- which
/// only the run time resolves.
fn runtime_alias_sources(compiler: &Compiler) -> crate::compiler::FSet<String> {
    let mut out: crate::compiler::FSet<String> = Default::default();
    for node in compiler.hir.nodes() {
        let HirNode::Call { name, args, .. } = node else {
            continue;
        };
        // `send`/`__send__`/`public_send` push the real name into the args.
        let source = match name.as_str() {
            "alias_method" => args.get(1),
            "send" | "__send__" | "public_send" => match literal_name(compiler, args.first()) {
                Some(n) if n == "alias_method" => args.get(2),
                _ => continue,
            },
            _ => continue,
        };
        if let Some(n) = literal_name(compiler, source) {
            out.insert(n);
        }
    }
    out
}

/// A `:sym` or `"str"` argument's text, or `None` for anything computed.
fn literal_name(compiler: &Compiler, arg: Option<&ArrayElem>) -> Option<String> {
    let ArrayElem::Single(id) = arg? else {
        return None;
    };
    match &compiler.hir[*id] {
        HirNode::SymbolLit(s) => Some(s.clone()),
        HirNode::StringLit(parts) => match parts.as_slice() {
            [crate::hir::StrPart::Lit(s)] => Some(s.clone()),
            _ => None,
        },
        _ => None,
    }
}
