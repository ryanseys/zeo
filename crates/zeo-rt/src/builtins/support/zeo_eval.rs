//! `Zeo::Eval` -- the runtime's own async snippet-compile surface.
//!
//! `Zeo::Eval.prepare(src, binding, file, line = 1, home = false)` begins
//! (or reports on) an off-thread compile of `src` under exactly the cache
//! key the later `Kernel.eval(src, binding, file, line)` will probe, and
//! answers `:ready`, `:compiling`, `:failed` or `:unsupported`. It is
//! idempotent -- a consumer polls it -- and never runs the snippet:
//! executing stays on the calling thread, in the real eval.
//!
//! `home` declares the future eval SITE's `has_home` (whether that call is
//! written in a scope with a block channel) -- a per-fiber thread-local
//! this caller cannot read for another site. The prepared key is
//! invalidated by any local-introducing eval on the same binding before
//! the paired eval runs: prepare and eval in lockstep per binding.

use crate::builtins::wrong_arg_type;
use crate::signal::Signal;
use crate::value::RubyValue;
use zeo_macros::ruby_module;

ruby_module! {
    Eval = zeo_abi::ZEO_EVAL_MODULE;

    def self."prepare"(_recv, src, binding, file, line?, home?) {
        prepare_impl(src, binding, file, line, home)
    }
}

fn prepare_impl(
    src: &RubyValue,
    binding: &RubyValue,
    file: &RubyValue,
    line: Option<&RubyValue>,
    home: Option<&RubyValue>,
) -> Result<RubyValue, Signal> {
    let Some(b) = crate::builtins::binding::as_binding(binding) else {
        return Err(wrong_arg_type(binding, "binding"));
    };
    let file = crate::builtins::convert::to_rstr(file)?
        .lock()
        .to_utf8_lossy()
        .into_owned();
    let line = match line {
        None => None,
        Some(RubyValue::Nil) => None,
        Some(v) => Some(crate::builtins::convert::to_index(v)? as u32),
    };
    let home = home.is_some_and(RubyValue::truthy);
    let status = crate::eval::prepare_with_binding(src, b, Some(file), line, home)?;
    let name = match status {
        crate::eval::PrepareStatus::Ready => "ready",
        crate::eval::PrepareStatus::Compiling => "compiling",
        crate::eval::PrepareStatus::Failed => "failed",
        crate::eval::PrepareStatus::Unsupported => "unsupported",
    };
    Ok(RubyValue::Symbol(crate::Symbol::intern(name)))
}
