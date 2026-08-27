//! `--dump=<kind>`'s report bodies: what the front end concluded about a
//! program, in a form a person can read.
//!
//! The two kinds here answer the two questions a loading failure raises, and
//! neither was answerable before: *which files became feature units, and
//! under which spellings* (`units`), and *which classes wait for a unit, and
//! does anything ever run their bodies* (`classes`). Both are front-end
//! facts, so both cost the analyze pass and nothing more -- seconds against
//! the minute a full compile of a 500-unit gem takes.
//!
//! The runtime half is `ZEO_RT_TRACE` in `zeo-rt`; the two are designed to be
//! read side by side, because a load bug is almost always a disagreement
//! between them.

use crate::analyze::Analyzed;
use crate::compiler::MarkerStream;

/// `--dump=units`: the compiled-in load path.
///
/// One block per unit: the file, every spelling a `require` can reach it
/// under, and how many statements its body runs. A DUPLICATE spelling across
/// two units is called out -- `zeo_rt::features::UNITS` is a map, so a
/// duplicate means one file silently answers a require written for another.
pub fn units(a: &Analyzed) -> String {
    let mut out = String::new();
    let mut claimed: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for (features, absolute, stmts) in &a.feature_units {
        out.push_str(&format!("{absolute}.rb\n"));
        for f in features {
            match claimed.get(f.as_str()) {
                Some(&winner) if winner != absolute.as_str() => out.push_str(&format!(
                    "  require {f:?}  COLLIDES with {winner}.rb (that one wins)\n"
                )),
                _ => {
                    claimed.entry(f).or_insert(absolute);
                    out.push_str(&format!("  require {f:?}\n"));
                }
            }
        }
        out.push_str(&format!("  {} statement(s)\n", stmts.len()));
    }
    for (feature, absolute, reason) in &a.declined_units {
        out.push_str(&format!("{absolute}.rb\n"));
        out.push_str(&format!("  require {feature:?}\n"));
        out.push_str(&format!("  DECLINED: {reason}\n"));
    }
    out.push_str(&format!(
        "{} unit(s), {} declined\n",
        a.feature_units.len(),
        a.declined_units.len()
    ));
    out
}

/// `--dump=classes`: every class, and what decides whether its constant
/// resolves.
///
/// A class registers for dispatch at startup whatever else is true of it;
/// what makes its CONSTANT exist is its body running. So each row carries
/// the two facts that settle that -- whether the constant is positional (the
/// class starts concealed and a body site must reveal it), and which
/// statement stream runs each of its sites.
///
/// `stream=NONE` is the row to look for: a positional class whose body no
/// stream reaches can never be revealed, so every read of it raises
/// `uninitialized constant` for a class the program plainly defines, while
/// `const_get` and `constants` still find it. That divergence is invisible
/// from Ruby and was worth an afternoon of bisection before this existed.
///
/// `only` filters by a substring of the fully-qualified name, because a
/// program that loads bundler has 1,600 classes.
pub fn classes(a: &Analyzed, only: Option<&str>) -> String {
    let compiler = &a.compiler;
    let mut tops: Vec<&[crate::hir::NodeId]> = vec![&a.main_statements];
    tops.extend(a.feature_units.iter().map(|(_, _, s)| s.as_slice()));
    let streams = compiler.class_marker_streams(&tops);

    let mut sites_by_class: std::collections::HashMap<u32, Vec<&crate::compiler::ClassBodySite>> =
        std::collections::HashMap::new();
    for site in &compiler.class_body_sites {
        sites_by_class.entry(site.class.0).or_default().push(site);
    }

    let mut out = String::new();
    let mut unreachable = 0usize;
    for (i, info) in compiler.classes.iter().enumerate() {
        let cid = crate::compiler::ClassId(i as u32);
        let fq = compiler.fq_name(cid);
        if only.is_some_and(|f| !fq.contains(f)) {
            continue;
        }
        let kind = match info.is_module {
            true => "module",
            false => "class",
        };
        let unit = match info.unit {
            Some(u) => format!("unit:{u}"),
            None => "-".to_string(),
        };
        let positional = compiler.constant_is_positional(cid);
        out.push_str(&format!(
            "{i} {fq} {kind} {unit}{}{}\n",
            match info.runtime_conditional {
                true => " conditional",
                false => "",
            },
            match positional {
                true => " positional",
                false => "",
            },
        ));
        let sites = sites_by_class.remove(&i_u32(i)).unwrap_or_default();
        if sites.is_empty() && positional {
            out.push_str("  NO BODY SITE -- nothing can ever reveal this class\n");
            unreachable += 1;
        }
        for site in sites {
            let where_ = site
                .def_node
                .and_then(|n| crate::analyze::source::source_location(compiler, n))
                .map_or_else(|| "?".to_string(), |(f, l)| format!("{f}:{l}"));
            // A site with no `def_node` is synthesized (a builtin's
            // registration shape), not a keyword the program wrote -- it has
            // no marker to reach and reports as such rather than as a hole.
            let stream = match site.def_node {
                None => "synthesized".to_string(),
                Some(n) => match streams.get(&n) {
                    Some(MarkerStream::Main) => "main".to_string(),
                    Some(MarkerStream::Unit(k)) => format!("unit:{k}"),
                    Some(MarkerStream::BlockDef) => "define_method-block".to_string(),
                    None => {
                        unreachable += 1;
                        "NONE -- no statement stream runs this body".to_string()
                    }
                },
            };
            // The statement KINDS, not just the count: a body that "kept one
            // statement" is a very different program depending on whether
            // that statement is the nested `class` keyword it was written
            // with or the bare constant read analyze replaced it by.
            let kinds: Vec<String> = site
                .stmts
                .iter()
                .map(|&n| node_kind(&compiler.hir[n]))
                .collect();
            out.push_str(&format!(
                "  site {where_} stream={stream} stmts=[{}]\n",
                kinds.join(", ")
            ));
        }
    }
    out.push_str(&format!(
        "{} class(es) listed, {} that nothing can reveal\n",
        compiler.classes.len(),
        unreachable
    ));
    out
}

/// Every class whose CONSTANT can never come into existence -- the invariant
/// behind `--dump=classes`' closing tally, callable on its own.
///
/// A class that waits for its unit registers for dispatch at startup and stays
/// CONCEALED until a class-body site reveals it. So a positional class with no
/// site that any statement stream reaches is a class the program defines and
/// no read of it can ever find: `uninitialized constant` from the compiled
/// read, while `const_get` and `constants` still answer, because those consult
/// the constant table rather than the conceal set.
///
/// That is what `class Git < Path` in `bundler/source/git.rb` became when an
/// unrelated `Bundler::Settings::Path` made the lowerer rewrite it into a
/// runtime `Git = Class.new(Path)`: the declaration lost its body site, only
/// `git_proxy.rb`'s bare reopen remained, and `Bundler::Source::Git` existed
/// twice. Nothing else in the pipeline said so -- no warning, no declined
/// unit, no compile error -- which is why this is an assertion and not a
/// report.
pub fn unrevealable_classes(a: &Analyzed) -> Vec<String> {
    let compiler = &a.compiler;
    let mut tops: Vec<&[crate::hir::NodeId]> = vec![&a.main_statements];
    tops.extend(a.feature_units.iter().map(|(_, _, s)| s.as_slice()));
    let streams = compiler.class_marker_streams(&tops);
    let reachable: std::collections::HashSet<u32> = compiler
        .class_body_sites
        .iter()
        // A site with no `def_node` is synthesized (a builtin's registration
        // shape), not a keyword the program wrote: it reveals nothing, and
        // nothing waits on it.
        .filter(|s| s.def_node.is_some_and(|n| streams.contains_key(&n)))
        .map(|s| s.class.0)
        .collect();
    compiler
        .classes
        .iter()
        .enumerate()
        // A gated BUILTIN is positional too, but its reveal rides its
        // `require` (`reveal_feature_classes`), not a body site -- `Zlib::
        // Deflate` has no site and needs none. Only the two shapes a SITE
        // reveals are asked about here.
        .filter(|(_, info)| info.unit.is_some() || info.runtime_conditional)
        .filter(|(i, _)| !reachable.contains(&(*i as u32)))
        .map(|(i, _)| compiler.fq_name(crate::compiler::ClassId(i as u32)))
        .collect()
}

/// `--dump=methods`: what materialization costs this program.
///
/// Inheriting a method is an ENTRY, not a copy: one `Scope` holds the body and
/// every class that inherits or includes it gets a `MethodEntry` naming that
/// same `ScopeId` (`analyze::mro::entry_for`). The backend is what multiplies
/// them -- `clif::classes` declares a body and a trampoline per entry, with
/// the owner baked into the symbol -- so one `def` becomes one emitted body
/// per class.
///
/// That is the monomorphization trade, and it is the largest single term in a
/// compiled program's size. Nothing reported it, so every figure about it so
/// far has been a division rather than a count.
///
/// A group is the classes sharing one `MethodEntry::def`, which is exactly
/// what the deleted rustc-era `analyze::share` bucketed. The closing tally is
/// the point; the rows above it name the widest `def`s, which is where any
/// sharing work would have to start.
pub fn methods(a: &Analyzed, top: usize) -> String {
    let compiler = &a.compiler;
    let mut by_def: std::collections::HashMap<u32, Vec<crate::compiler::ClassId>> =
        std::collections::HashMap::new();
    // `ancestors` already starts with the class itself, so this is Object,
    // Kernel and BasicObject -- the tail every ancestry ends with.
    let universal: &[crate::compiler::ClassId] =
        &compiler.class(crate::compiler::OBJECT_CLASS).ancestors;
    let (mut entries, mut carried, mut shared) = (0usize, 0usize, 0usize);
    for (i, info) in compiler.classes.iter().enumerate() {
        let cid = crate::compiler::ClassId(i_u32(i));
        // Only an ORDINARY class emits a body per entry. `Object`'s own
        // instance methods are emitted once by `clif::collect::collect_methods`
        // however many classes carry them, and a builtin or bootstrap class
        // emits only its DELTAS (`clif::classes`) -- a top-level `def` reaching
        // every exception through Object is deliberately nobody's delta. So
        // those entries are real dispatch rows that cost no code, and counting
        // them as copies is what turns this report into a wrong number.
        let emits = !(info.is_builtin || info.is_bootstrap || i == 0);
        for entry in info.methods.iter().chain(&info.class_methods) {
            if !emits {
                carried += 1;
                continue;
            }
            // ... and an INHERITED definition from the universal spine now
            // reuses the one body `collect_methods` emits on Object, so it
            // costs no code either (`clif::classes`). Mirrors that guard;
            // `accessor_shape` is the one arm it cannot ask about here, and an
            // accessor on Object is a shape no bundled gem writes.
            let scope = compiler.scope(entry.def);
            if !info.own_methods.contains(&entry.def)
                && universal.contains(&scope.defining_class)
                && info.box_id == 0
            {
                shared += 1;
                continue;
            }
            entries += 1;
            by_def.entry(entry.def.0).or_default().push(cid);
        }
    }

    let mut widest: Vec<(usize, u32, &Vec<crate::compiler::ClassId>)> = by_def
        .iter()
        .map(|(&def, owners)| (owners.len(), def, owners))
        .filter(|(n, _, _)| *n > 1)
        .collect();
    // By width, then by `def`, so two definitions carried the same number of
    // times report in a stable order rather than the map's.
    widest.sort_by(|x, y| y.0.cmp(&x.0).then(x.1.cmp(&y.1)));

    let mut out = String::new();
    for &(n, def, owners) in widest.iter().take(top) {
        let scope = compiler.scope(crate::compiler::ScopeId(def));
        let where_ = scope
            .def_node
            .and_then(|node| crate::analyze::source::source_location(compiler, node))
            .map_or_else(|| "?".to_string(), |(f, l)| format!("{f}:{l}"));
        out.push_str(&format!(
            "{n:>5} classes  {}#{}  {where_}\n",
            compiler.fq_name(scope.defining_class),
            scope.name,
        ));
        let sample: Vec<String> = owners.iter().take(4).map(|&c| compiler.fq_name(c)).collect();
        out.push_str(&format!(
            "         {}{}\n",
            sample.join(", "),
            match owners.len() > sample.len() {
                true => format!(", +{} more", owners.len() - sample.len()),
                false => String::new(),
            }
        ));
    }
    let extra: usize = widest.iter().map(|&(n, _, _)| n - 1).sum();
    out.push_str(&format!(
        "{entries} emitted bod(ies) over {} definition(s); {carried} more entr(ies) \
         are carried by Object or a builtin and emit nothing\n",
        by_def.len()
    ));
    out.push_str(&format!(
        "{} definition(s) reach more than one class, costing {extra} extra bod(ies) \
         -- {:.0}% of what is emitted\n",
        widest.len(),
        match entries {
            0 => 0.0,
            n => extra as f64 / n as f64 * 100.0,
        }
    ));
    out.push_str(&format!(
        "{shared} row(s) inherit a definition on {} -- every class's tail -- and \
         SHARE the one body emitted for it\n",
        universal
            .iter()
            .map(|&c| compiler.fq_name(c))
            .collect::<Vec<_>>()
            .join("/")
    ));
    out
}

fn i_u32(i: usize) -> u32 {
    i as u32
}

/// A HIR node's variant name, plus the name it carries where that is what
/// identifies it. `Debug` on the node itself would print whole subtrees.
fn node_kind(node: &crate::hir::HirNode) -> String {
    use crate::hir::HirNode;
    match node {
        HirNode::ClassDef { name, .. } => format!("ClassDef({name})"),
        HirNode::ClassRef(name) => format!("ClassRef({name})"),
        HirNode::Call { name, .. } => format!("Call({name})"),
        HirNode::ConstWrite { name, .. } => format!("ConstWrite({name})"),
        HirNode::Include(name) => format!("Include({name})"),
        other => {
            let text = format!("{other:?}");
            let end = text
                .find(|c: char| !c.is_alphanumeric() && c != '_')
                .unwrap_or(text.len());
            text[..end].to_string()
        }
    }
}
