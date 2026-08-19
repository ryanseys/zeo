//! `zeo_rt_bind_params`: the trampoline path's argument binder. Routes a
//! dynamic call's `argv` into a compiled method's signature slots from its
//! `.rodata` [`ParamDescC`] -- composing the pieces that already exist
//! (the kwargs peel in [`crate::dispatch::bind_dynamic_kwargs`], the
//! `**nil` refusal, the arity shapes of `wrong_arity`) rather than
//! reimplementing them. The direct compiled->compiled path never comes
//! here; its routing is static.

use crate::{RubyValue, Signal};
use zeo_abi::abi::{
    KwParamC, PARAM_STAR_ANON, PARAM_STAR_NAMED, PARAM_STAR_NONE, ParamDescC, STATUS_OK,
    STATUS_SIGNAL,
};

/// Bind `argv` to `desc`'s signature slots.
///
/// `slots` is the trampoline's stack array of `n_slots` value cells
/// (required + optional + named-rest + post + keywords + named-kwrest, in
/// slot order); every cell is written -- `Nil` first (so an error path
/// releases safely), then an OWNED value per bound slot. `present` gets a
/// slot-indexed bitmap: bit `s` set = slot `s` holds a bound value (an
/// absent optional's bit stays clear; its default runs in the body). The
/// binder's raises (wrong arity, keyword errors) run under the CALLEE
/// frame `desc` carries, exactly like the rustc trampolines.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_bind_params(
    desc: *const ParamDescC,
    argv: *const RubyValue,
    argc: usize,
    slots: *mut RubyValue,
    present: *mut u64,
) -> i32 {
    let desc = unsafe { &*desc };
    let n_slots = slot_count(desc);
    for i in 0..n_slots {
        unsafe { slots.add(i).write(RubyValue::Nil) };
    }
    unsafe { present.write(0) };
    let args: &[RubyValue] = if argc == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(argv, argc) }
    };

    // The callee frame, pushed around the whole binding (CRuby attributes
    // argument errors to the def line) and popped before returning either
    // way -- the body pushes its own.
    let file = unsafe { super::static_str(desc.file.ptr, desc.file.len) };
    let label = unsafe { super::static_str(desc.label.ptr, desc.label.len) };
    crate::frames::frame_push_raw(file, label, desc.line, desc.end_line);
    let result = unsafe { bind(desc, n_slots, args, slots, present) };
    crate::frames::frame_pop_raw();
    match result {
        Ok(()) => STATUS_OK,
        Err(sig) => {
            crate::signal::set_pending(sig);
            STATUS_SIGNAL
        }
    }
}

/// The signature-slot count `desc` implies (what the trampoline's stack
/// array holds; the block channel is separate).
fn slot_count(desc: &ParamDescC) -> usize {
    desc.nreq as usize
        + desc.nopt as usize
        + usize::from(desc.rest == PARAM_STAR_NAMED)
        + desc.npost as usize
        + desc.n_kws
        + usize::from(desc.kwrest == PARAM_STAR_NAMED)
}

unsafe fn bind(
    desc: &ParamDescC,
    n_slots: usize,
    args: &[RubyValue],
    slots: *mut RubyValue,
    present: *mut u64,
) -> Result<(), Signal> {
    let name = unsafe { super::str_slice(desc.name.ptr, desc.name.len) };
    let kws: &[KwParamC] = if desc.n_kws == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(desc.kws, desc.n_kws) }
    };
    let kw_names: Vec<&str> = kws
        .iter()
        .map(|k| unsafe { super::str_slice(k.name.ptr, k.name.len) })
        .collect();
    let has_kwrest = desc.kwrest != PARAM_STAR_NONE;

    // Keyword split first (`**nil` refuses BEFORE the arity check; a
    // keyword-declaring callee peels the marked trailing Hash; a
    // keywordless one keeps it positional -- the options-hash idiom).
    let (positional, kw_req_vals, kw_opt_vals, kw_rest_pairs) = if desc.no_keywords != 0 {
        crate::dispatch::reject_marked_kwargs(args)?;
        (args, Vec::new(), Vec::new(), Vec::new())
    } else if !kws.is_empty() || has_kwrest {
        let req: Vec<&str> = kws
            .iter()
            .zip(&kw_names)
            .filter(|(k, _)| k.required != 0)
            .map(|(_, n)| *n)
            .collect();
        let opt: Vec<&str> = kws
            .iter()
            .zip(&kw_names)
            .filter(|(k, _)| k.required == 0)
            .map(|(_, n)| *n)
            .collect();
        crate::dispatch::bind_dynamic_kwargs(name, args, &req, &opt, has_kwrest)?
    } else {
        (args, Vec::new(), Vec::new(), Vec::new())
    };

    // Arity, on the positionals that remain after the peel.
    let (nreq, nopt, npost) = (desc.nreq as usize, desc.nopt as usize, desc.npost as usize);
    let n_pos = positional.len();
    let min = nreq + npost;
    let has_rest = desc.rest != PARAM_STAR_NONE;
    if n_pos < min || (!has_rest && n_pos > nreq + nopt + npost) {
        let expected = if has_rest {
            format!("{min}+")
        } else if nopt == 0 {
            format!("{min}")
        } else {
            format!("{min}..{}", nreq + nopt + npost)
        };
        return Err(crate::dispatch::wrong_arity(n_pos, &expected));
    }

    // Route. Every bound slot gets an OWNED value + its presence bit; the
    // written values are boundary crossings into compiled-code storage
    // (the trampoline releases them after the body call).
    let mut f = Filler {
        s: 0,
        n_slots,
        slots,
        present,
    };
    let extra = n_pos - min;
    let opt_bound = extra.min(nopt);
    for v in &positional[..nreq] {
        f.fill(v.clone());
    }
    for i in 0..nopt {
        if i < opt_bound {
            f.fill(positional[nreq + i].clone());
        } else {
            f.skip(); // absent: slot stays Nil, bit stays clear
        }
    }
    match desc.rest {
        PARAM_STAR_NAMED => {
            let elems = positional[nreq + opt_bound..n_pos - npost].to_vec();
            f.fill(RubyValue::Array(crate::value::collections::array_new(
                elems,
            )));
        }
        PARAM_STAR_ANON | PARAM_STAR_NONE => {}
        other => unreachable!("ParamDescC.rest kind {other}"),
    }
    for v in &positional[n_pos - npost..] {
        f.fill(v.clone());
    }
    let (mut ri, mut oi) = (0usize, 0usize);
    for kw in kws {
        if kw.required != 0 {
            f.fill(kw_req_vals[ri].clone());
            ri += 1;
        } else {
            match &kw_opt_vals[oi] {
                Some(v) => f.fill(v.clone()),
                None => f.skip(),
            }
            oi += 1;
        }
    }
    match desc.kwrest {
        PARAM_STAR_NAMED => {
            f.fill(RubyValue::Hash(crate::value::collections::hash_new(
                kw_rest_pairs,
            )));
        }
        PARAM_STAR_ANON | PARAM_STAR_NONE => {}
        other => unreachable!("ParamDescC.kwrest kind {other}"),
    }
    debug_assert_eq!(f.s, n_slots, "slot routing must cover the whole layout");
    Ok(())
}

/// The slot writer: owned value in, presence bit set, cursor advanced.
struct Filler {
    s: usize,
    n_slots: usize,
    slots: *mut RubyValue,
    present: *mut u64,
}

impl Filler {
    fn fill(&mut self, v: RubyValue) {
        debug_assert!(self.s < self.n_slots, "slot routing overran the layout");
        super::leakcheck::created(&v);
        unsafe {
            self.slots.add(self.s).write(v);
            *self.present |= 1 << self.s;
        }
        self.s += 1;
    }

    fn skip(&mut self) {
        self.s += 1;
    }
}
