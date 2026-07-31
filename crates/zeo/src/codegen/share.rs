//! Emit one body for a `def` that many classes inherited, instead of one per
//! class.
//!
//! `analyze::share` finds the candidate groups; this decides which of them
//! actually share, and emits the shared bodies. The decision is made by
//! EMITTING, not by a checklist: every member's body is emitted in the shared
//! form and the group shares only when the results are token-identical. A rule
//! this file forgot therefore costs a missed sharing opportunity, never a wrong
//! program -- and `ZEO_VERIFY_SHARE=1` turns the same comparison into a hard
//! error so a regression in the property is loud rather than silent.
//!
//! The shared form is the one `emit_builtin_method_fn` has always used for a
//! reopened builtin and for every top-level `def`: a free function over
//! `__self: RubyValue`. A `RubyValue` receiver is what lets a single body serve
//! classes whose concrete structs differ, and it makes an implicit-self call
//! dispatch on the receiver actually passed rather than on a class the body
//! cannot know.
//!
//! Each class still gets a one-line `def` that forwards. That keeps `super`,
//! `Method#owner`, the dispatch table, visibility and every Path-1 call site
//! working verbatim, and it costs nothing at runtime: the wrapper CONSUMES its
//! `Arc<Self>`, so `RubyValue::Object(Self::new_handle(self))` is an unsize
//! coercion with no reference-count traffic at all.

use crate::compiler::{ClassId, Compiler, ScopeId};
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote};
use std::collections::HashMap;

/// A body under this many emitted bytes stays materialized. Small bodies are
/// accessors and one-liners: they are what LLVM most wants to inline into a
/// caller, and they are the cheapest duplicates to keep. The gate is the
/// profile-free hot/cold split -- in prism, minitest and uri, bodies at or over
/// this size are 85-98% of all duplicated bytes.
const SIZE_GATE: usize = 500;

pub(crate) struct SharedBodies {
    /// Every member scope of a sharing group, mapped to the group's function.
    by_scope: HashMap<ScopeId, Ident>,
    /// The `__sh` container, absent when nothing shares.
    container: Option<TokenStream>,
}

impl SharedBodies {
    /// The shared function serving `sid`, if its group shares.
    pub(crate) fn call(&self, sid: ScopeId) -> Option<&Ident> {
        self.by_scope.get(&sid)
    }

    pub(crate) fn container(&self) -> Option<&TokenStream> {
        self.container.as_ref()
    }

    /// `ZEO_SHARE=0` restores per-class materialization byte for byte -- a
    /// one-line field diagnosis for anything this file is blamed for.
    fn disabled() -> bool {
        static D: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *D.get_or_init(|| std::env::var("ZEO_SHARE").is_ok_and(|v| v == "0"))
    }

    pub(crate) fn plan(compiler: &Compiler) -> Self {
        if Self::disabled() {
            return SharedBodies {
                by_scope: HashMap::new(),
                container: None,
            };
        }
        let verify = crate::analyze::share::verify_enabled();
        let mut by_scope = HashMap::new();
        let mut fns: Vec<TokenStream> = Vec::new();
        let (mut agreed, mut differed) = (0usize, 0usize);
        let (mut rejected, mut too_small, mut copies) = (0usize, 0usize, 0usize);
        let mut report = String::new();

        let candidates = crate::analyze::share::groups(compiler);
        let candidate_copies: usize = candidates.iter().map(|g| g.members.len() - 1).sum();
        for group in candidates {
            if !shareable(compiler, &group) {
                rejected += 1;
                continue;
            }
            let fn_ident = format_ident!("__sh{}", fns.len());
            let mut rendered: Vec<(ClassId, String)> = Vec::with_capacity(group.members.len());
            let mut bodies = Vec::with_capacity(group.members.len());
            for &(cid, sid) in &group.members {
                let body = super::emit_value_self_method_fn(compiler, cid, sid, &fn_ident);
                rendered.push((cid, body.to_string()));
                bodies.push(body);
            }
            let (base_cid, base) = &rendered[0];
            if let Some((other_cid, other)) = rendered[1..].iter().find(|(_, r)| r != base) {
                differed += 1;
                if verify && report.len() < 8000 {
                    report += &format!(
                        "{}#{} differs between {} and {}\n  {}\n  {}\n",
                        compiler.fq_name(group.defining_class),
                        group.name,
                        compiler.fq_name(*base_cid),
                        compiler.fq_name(*other_cid),
                        first_divergence(base, other),
                        first_divergence(other, base),
                    );
                }
                continue;
            }
            if base.len() < SIZE_GATE {
                too_small += 1;
                continue;
            }
            agreed += 1;
            copies += group.members.len() - 1;
            for &(_, sid) in &group.members {
                by_scope.insert(sid, fn_ident.clone());
            }
            fns.push(bodies.swap_remove(0));
        }

        if verify {
            eprintln!(
                "zeo-verify-share: {agreed} of {} groups share ({copies} of \
                 {candidate_copies} duplicate bodies removed); rejected {rejected}, \
                 under the size gate {too_small}, disagreed {differed}",
                agreed + rejected + too_small + differed,
            );
            if differed > 0 {
                crate::codegen::record_unsupported(format!(
                    "ZEO_VERIFY_SHARE found {differed} disagreeing group(s):\n{report}"
                ));
            }
        }
        let container = (!fns.is_empty()).then(|| {
            quote! {
                #[allow(non_snake_case)]
                pub mod __sh { #[allow(unused_imports)] use super::*; #(#fns)* }
            }
        });
        SharedBodies {
            by_scope,
            container,
        }
    }
}

/// The rules that hold before anything is emitted. Everything else is decided
/// by comparing the emissions themselves.
fn shareable(compiler: &Compiler, group: &crate::analyze::share::Group) -> bool {
    let scope = compiler.scope(group.members[0].1);
    // A pristine built-in exception body is served by `zeo-rt`'s own
    // `register_exceptions`, so codegen emits no copy of it to share.
    if scope.native_default {
        return false;
    }
    // An `undef_method` zeo could not decide at compile time means the name may
    // not resolve here at runtime, so the class keeps its own emitted copy for
    // the dynamic path to tombstone.
    if group
        .members
        .iter()
        .any(|&(cid, _)| compiler.may_be_undefined_at_runtime(cid, &group.name))
    {
        return false;
    }
    // A `RubyValue` receiver reaches instance variables BY NAME, which is a
    // linear scan where the class's own body indexes a slot. Deferred until
    // slot-indexed access exists on a dynamic receiver.
    let mut ivars = Vec::new();
    for &n in &scope.body {
        crate::analyze::collect_ivars(&compiler.hir, n, &mut ivars);
    }
    for id in scope.params.default_ids() {
        crate::analyze::collect_ivars(&compiler.hir, id, &mut ivars);
    }
    ivars.is_empty()
}

/// A short window of `a` around the first token where it parts from `b`.
fn first_divergence(a: &str, b: &str) -> String {
    let (av, bv): (Vec<&str>, Vec<&str>) = (a.split(' ').collect(), b.split(' ').collect());
    let at = av
        .iter()
        .zip(&bv)
        .position(|(x, y)| x != y)
        .unwrap_or(av.len().min(bv.len()));
    av[at.saturating_sub(8)..(at + 8).min(av.len())].join(" ")
}
