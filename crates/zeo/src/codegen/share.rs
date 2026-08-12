//! Emit one body for a `def` that many classes inherited, instead of one per
//! class.
//!
//! `analyze::share` finds the candidate groups -- one `def`, and every class
//! whose method entry points at it. This decides which of them can be served by
//! a single emitted function.
//!
//! The decision used to be made BY EMITTING: every member's body was rendered
//! in the shared form and the group shared only when the token strings all
//! matched. That is correct, and it is also exactly the `O(classes x methods)`
//! cost sharing exists to remove -- active_model carries 3,624 definitions
//! behind 22,623 entries, so 19,000 of those renders were thrown away.
//!
//! Now ONE member is emitted, with the emitter recording every question it asks
//! about its receiver class (`codegen::class_query`), and the rest are settled
//! by REPLAYING those questions. A class that answers them all the same way
//! would have rendered the same tokens; one that does not gets its own body.
//! `ZEO_VERIFY_SHARE=1` still emits everything and compares, and now asserts the
//! property that makes the replay sound: a member whose render differs must
//! have a recorded question that explains it.
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

use super::class_query::Trace;
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

/// Whether `body` renders to fewer than `limit` bytes -- the same answer
/// `body.to_string().len() < limit` gives, without the string.
///
/// This is asked of EVERY candidate shared body, and the bodies it is asked
/// about are the large ones by construction, so materializing each one just to
/// measure it and then dropping it was megabytes of pure allocation churn per
/// compile. Formatting into a counter is exact rather than an estimate --
/// which matters, because an estimate that disagreed near the threshold would
/// silently move sharing decisions.
///
/// The counter refuses once it passes `limit`, which aborts the formatting:
/// nothing after that can change the answer, and a body far over the gate is
/// the case worth not rendering.
fn shorter_than(body: &TokenStream, limit: usize) -> bool {
    use std::fmt::Write as _;

    struct Counter {
        len: usize,
        limit: usize,
    }
    impl std::fmt::Write for Counter {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            self.len += s.len();
            // `Err` is how a `fmt::Write` sink says "stop"; the caller reads
            // `len`, not the result.
            if self.len >= self.limit {
                return Err(std::fmt::Error);
            }
            Ok(())
        }
    }

    let mut counter = Counter { len: 0, limit };
    let _ = write!(counter, "{body}");
    counter.len < limit
}

pub(crate) struct SharedBodies {
    /// Which shared function serves a given class's copy of a definition.
    ///
    /// Keyed by BOTH, not by the definition alone: a group whose members
    /// disagree splits into more than one emitted body, and each class must
    /// reach the one its own answers produced.
    by_class_def: HashMap<(ClassId, ScopeId), Ident>,
    /// The `__sh` container, absent when nothing shares.
    container: Option<TokenStream>,
}

/// One emitted body, and the classes it serves.
struct Bucket {
    trace: Trace,
    classes: Vec<ClassId>,
    body: TokenStream,
    ident: Ident,
}

impl SharedBodies {
    /// The shared function serving `cid`'s copy of `sid`, if it has one.
    pub(crate) fn call(&self, cid: ClassId, sid: ScopeId) -> Option<&Ident> {
        self.by_class_def.get(&(cid, sid))
    }

    pub(crate) fn container(&self) -> Option<&TokenStream> {
        self.container.as_ref()
    }

    /// `ZEO_DEBUG=no-share` restores per-class materialization -- a one-line
    /// field diagnosis for anything this file is blamed for.
    fn disabled() -> bool {
        crate::debug_flags::debug(crate::debug_flags::DebugFlag::NoShare)
    }

    pub(crate) fn plan(compiler: &Compiler) -> Self {
        if Self::disabled() {
            return SharedBodies {
                by_class_def: HashMap::new(),
                container: None,
            };
        }
        let verify = crate::analyze::share::verify_enabled();
        let mut by_class_def = HashMap::new();
        let mut fns: Vec<TokenStream> = Vec::new();
        let mut stats = Stats::default();

        for group in crate::analyze::share::groups(compiler) {
            stats.candidate_copies += group.members.len() - 1;
            if !shareable(compiler, &group) {
                stats.rejected += 1;
                continue;
            }
            let buckets = bucket_members(compiler, &group);
            stats.queries += buckets.iter().map(|b| b.trace.len()).sum::<usize>();
            stats.emissions += buckets.len();
            if buckets.len() > 1 {
                stats.split += 1;
            }
            if verify {
                verify_buckets(compiler, &group, &buckets, &mut stats);
            }
            for bucket in buckets {
                // A bucket of one has nothing to share: the class would emit
                // this body into its own `impl` anyway, and `emit_class` does
                // that for any class not mapped here.
                if bucket.classes.len() < 2 {
                    stats.alone += 1;
                    continue;
                }
                if shorter_than(&bucket.body, SIZE_GATE) {
                    stats.too_small += 1;
                    continue;
                }
                stats.agreed += 1;
                stats.copies += bucket.classes.len() - 1;
                for cid in bucket.classes {
                    by_class_def.insert((cid, group.def), bucket.ident.clone());
                }
                fns.push(bucket.body);
            }
        }

        if verify {
            stats.report();
        }
        let container = (!fns.is_empty()).then(|| {
            quote! {
                #[allow(non_snake_case)]
                pub mod __sh { #[allow(unused_imports)] use super::*; #(#fns)* }
            }
        });
        SharedBodies {
            by_class_def,
            container,
        }
    }
}

/// Emit as few bodies as the group's members actually need.
///
/// The first member is emitted with its questions recorded; each member after
/// it joins the first bucket whose trace it agrees with, and only mints a new
/// one (with a new emission) when it agrees with none. For the overwhelming
/// majority of groups that is one emission for the whole group, however many
/// classes it holds.
fn bucket_members(compiler: &Compiler, group: &crate::analyze::share::Group) -> Vec<Bucket> {
    let mut buckets: Vec<Bucket> = Vec::new();
    for &cid in &group.members {
        if let Some(bucket) = buckets
            .iter_mut()
            .find(|b| b.trace.agrees_for(compiler, cid))
        {
            bucket.classes.push(cid);
            continue;
        }
        // Named after the definition it serves rather than a running counter,
        // so a body's name does not shift when an unrelated group stops
        // sharing.
        let ident = format_ident!("__sh{}_{}", group.def.0, buckets.len());
        let trace = std::cell::RefCell::new(Trace::default());
        // `self_slots` is not the caller's choice, it is the RECEIVER's: only a
        // class with a generated struct has an ivar layout to index into. Asked
        // through the trace so a group holding both a user class and a reopened
        // builtin splits on it rather than emitting one body that is wrong for
        // half its members.
        let self_slots = super::Ctx::ask_class(
            compiler,
            cid,
            Some(&trace),
            super::class_query::ClassQuery::HasStruct,
        )
        .yes();
        let body = super::emit_value_self_method_fn(
            compiler,
            cid,
            group.def,
            &ident,
            self_slots,
            true,
            Some(&trace),
        );
        buckets.push(Bucket {
            trace: trace.into_inner(),
            classes: vec![cid],
            body,
            ident,
        });
    }
    buckets
}

/// `ZEO_VERIFY_SHARE=1`: emit every member the old way and hold the buckets to
/// what the comparison says.
///
/// Two directions, and only one of them is a bug. A member that renders
/// DIFFERENTLY from its bucket's body is a per-class read the emitter makes
/// without going through `Ctx::ask` -- the trace could not see it, so the
/// replay put the class in a bucket whose body is wrong for it. A member that
/// renders the SAME as another bucket's body is only a missed opportunity: the
/// trace recorded a question that turned out not to matter here.
fn verify_buckets(
    compiler: &Compiler,
    group: &crate::analyze::share::Group,
    buckets: &[Bucket],
    stats: &mut Stats,
) {
    let name = &compiler.scope(group.def).name;
    let owner = compiler.fq_name(compiler.scope(group.def).defining_class);
    for bucket in buckets {
        let expected = bucket.body.to_string();
        for &cid in &bucket.classes {
            // Re-emitted exactly as bucketing would have: `self_slots` off the
            // receiver, `shared` on. Hardcoding either would compare against a
            // body no member was ever going to get.
            let self_slots = compiler.has_generated_struct(cid);
            let actual = super::emit_value_self_method_fn(
                compiler,
                cid,
                group.def,
                &bucket.ident,
                self_slots,
                true,
                None,
            )
            .to_string();
            if actual == expected {
                continue;
            }
            stats.untraced += 1;
            if stats.report_text.len() < 8000 {
                stats.report_text += &format!(
                    "{owner}#{name} on {} renders differently from the body its bucket \
                     emitted, but every recorded question agreed -- a per-class read that \
                     does not go through `Ctx::ask`\n  {}\n  {}\n",
                    compiler.fq_name(cid),
                    first_divergence(&expected, &actual),
                    first_divergence(&actual, &expected),
                );
            }
        }
    }
    // Why each split happened, so a bucket count above 1 is explainable rather
    // than mysterious.
    if buckets.len() > 1 && stats.report_text.len() < 8000 {
        for bucket in &buckets[1..] {
            if let Some((q, mine, theirs)) = buckets[0]
                .trace
                .first_disagreement(compiler, bucket.classes[0])
            {
                stats.report_text += &format!(
                    "{owner}#{name} splits on {}: {q:?} answered {mine:?} vs {theirs:?}\n",
                    compiler.fq_name(bucket.classes[0]),
                );
            }
        }
    }
}

#[derive(Default)]
struct Stats {
    candidate_copies: usize,
    rejected: usize,
    /// Buckets holding a single class -- a split left it on its own, so there
    /// is nothing for it to share with.
    alone: usize,
    too_small: usize,
    agreed: usize,
    copies: usize,
    queries: usize,
    emissions: usize,
    split: usize,
    untraced: usize,
    report_text: String,
}

impl Stats {
    fn report(&self) {
        eprintln!(
            "zeo-verify-share: {} bodies serve {} classes ({} of {} duplicate bodies \
             removed); rejected {}, under the size gate {}, left alone by a split {}, \
             groups split {}; {} emissions, {} recorded question(s), \
             {} unrecorded per-class read(s)",
            self.agreed,
            self.agreed + self.copies,
            self.copies,
            self.candidate_copies,
            self.rejected,
            self.too_small,
            self.alone,
            self.split,
            self.emissions,
            self.queries,
            self.untraced,
        );
        if !self.report_text.is_empty() {
            eprintln!("{}", self.report_text);
        }
        if self.untraced > 0 {
            crate::codegen::record_unsupported(format!(
                "ZEO_VERIFY_SHARE found {} class(es) served a body that does not match what \
                 they would have emitted, with no recorded question to explain it -- a \
                 per-class read that does not go through `Ctx::ask`:\n{}",
                self.untraced, self.report_text
            ));
        }
    }
}

/// The rules that hold before anything is emitted.
fn shareable(compiler: &Compiler, group: &crate::analyze::share::Group) -> bool {
    let scope = compiler.scope(group.def);
    // A pristine built-in exception body is served by `zeo-rt`'s own
    // `register_exceptions`, so codegen emits no copy of it to share.
    if scope.native_default {
        return false;
    }
    // An `undef_method` zeo could not decide at compile time means the name may
    // not resolve here at runtime, so the class keeps its own emitted copy for
    // the dynamic path to tombstone.
    !group
        .members
        .iter()
        .any(|&cid| compiler.may_be_undefined_at_runtime(cid, &scope.name))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of `shorter_than` is that it is not an estimate: a
    /// disagreement with the string it replaced would silently move which
    /// bodies get shared, and nothing downstream would say so.
    #[test]
    fn the_counter_agrees_with_the_string_it_replaced() {
        let bodies = [
            quote! {},
            quote! { 1 },
            quote! { let x = 1; x + 2 },
            quote! {
                fn f(a: RubyValue, b: RubyValue) -> Result<RubyValue, Signal> {
                    let mut acc = zeo_rt::RubyValue::Int(0);
                    for _ in 0..10 { acc = zeo_rt::add(acc, a.clone())?; }
                    Ok(acc)
                }
            },
        ];
        for body in bodies {
            let rendered = body.to_string().len();
            // Sweep the threshold across and past the real length, so the
            // boundary itself is covered rather than assumed.
            for limit in 0..rendered + 4 {
                assert_eq!(
                    shorter_than(&body, limit),
                    rendered < limit,
                    "limit {limit} on a {rendered}-byte body: {body}"
                );
            }
        }
    }
}
