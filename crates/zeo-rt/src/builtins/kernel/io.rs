//! `Kernel`'s output surface -- `puts`, `print`, `p`, `pp`, `warn`,
//! `format`/`printf` -- and the `$stdin` forwarder. The `ruby_module!` rows
//! stay in `mod.rs` and call these by bare name.

use super::*;

/// `gets`/`readline`/`readlines` against `$stdin` -- see their rows.
pub(super) fn stdin_send(name: &str, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    let stdin = crate::globals::global_get(0, "$stdin");
    crate::dispatch::send_value(&stdin, Symbol::intern(name), args, None)
}

/// `Kernel#puts`: zero args print one newline; arrays flatten recursively,
/// each scalar on its own line (nil renders empty) -- CRuby's exact rules.
/// Routed through whatever `$stdout` currently holds (default: the
/// `STDOUT` singleton) -- `$stdout = STDERR` or any duck-typed writer
/// redirects the whole print family. See `builtins::io` for the rendering
/// and write plumbing.
pub fn kernel_puts(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    let mut buf = Vec::new();
    // A raising `to_s` mid-render still FLUSHES what rendered before it --
    // CRuby writes line by line, so `puts [1, raiser]` prints "1" and then
    // raises (oracle-verified). Rendering into one buffer and flushing
    // before propagating reproduces that observable order.
    let rendered = crate::builtins::io::render_puts(args, &mut buf);
    crate::builtins::io::write_bytes(&crate::builtins::io::current_stdout(), &buf)?;
    rendered?;
    Ok(RubyValue::Nil)
}

/// `Kernel#warn`: writes each message on its own line to `$stderr` and
/// returns nil. It does not double a trailing newline, the same rule `puts`
/// follows. The `uplevel:` keyword is not modelled, because kwargs never
/// reach the Kernel-function path.
pub fn kernel_warn(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    // A trailing keyword Hash (`category:`/`uplevel:`) is consumed, not printed.
    // CRuby leaves `Warning[:deprecated]` off by default (so a :deprecated
    // warning prints nothing), while :experimental and every other category
    // are on. The caller already evaluated the message arguments, so their
    // side effects happen regardless of suppression.
    let mut msgs = args;
    let mut uplevel: Option<usize> = None;
    if let Some(RubyValue::Hash(h)) = args.last() {
        let cat_key = RubyValue::Symbol(crate::Symbol::intern("category"));
        let up_key = RubyValue::Symbol(crate::Symbol::intern("uplevel"));
        let pairs = crate::hash_pairs(h);
        let is_kwargs = !pairs.is_empty()
            && pairs
                .iter()
                .all(|(k, _)| k.rb_eq(&cat_key) || k.rb_eq(&up_key));
        if is_kwargs {
            msgs = &args[..args.len() - 1];
            if let RubyValue::Symbol(s) = crate::hash_get(h, &cat_key)
                && s.name() == "deprecated"
            {
                return Ok(RubyValue::Nil);
            }
            if let RubyValue::Int(n) = crate::hash_get(h, &up_key)
                && n >= 0
            {
                uplevel = Some(n as usize);
            }
        }
    }
    let mut buf = Vec::new();
    // `uplevel: n` prefixes the first message with "file:line: warning: "
    // from the caller frame n levels up. This bare fn is folded into its
    // caller (no frame of its own), so the TOP frame is uplevel 0.
    if let Some(n) = uplevel
        && let Some(&(file, line, _)) = crate::frames::caller_frames(0).get(n)
    {
        buf.extend_from_slice(format!("{file}:{line}: warning: ").as_bytes());
    }
    // `rb_warn_m` renders its messages with `rb_io_puts` into a temp string,
    // so `warn` IS `puts` on stderr: an Array is one line per element
    // (recursively), an empty Array writes nothing, and a trailing newline is
    // never doubled. NO partial flush on a raising `to_s` -- CRuby renders
    // the whole message before its one write (unlike `puts`/`print`/`p`), so
    // nothing reaches stderr. And with no messages at all there is no write,
    // where bare `puts` would emit a newline.
    if msgs.is_empty() {
        return Ok(RubyValue::Nil);
    }
    crate::builtins::io::render_puts(msgs, &mut buf)?;
    crate::builtins::io::write_bytes(&crate::builtins::io::current_stderr(), &buf)?;
    Ok(RubyValue::Nil)
}

/// `Kernel#p`: each argument's INSPECT rendering on its own line; returns
/// nil / the single argument / the argument array (CRuby's exact shapes).
pub fn kernel_p(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    // CRuby's `rb_f_p` is a C frame: an `inspect` that raises shows
    // `in 'Kernel#p'` under the raising frame. Statically-emitted calls
    // bypass the dispatch boundary, so the frame is placed here.
    let _frame = crate::frames::synthetic_c_frame("Kernel#p");
    let mut buf = String::new();
    // Fallible: a raising user `inspect` propagates out of `p` (catchable,
    // CRuby's rule) -- after flushing the args already rendered, since
    // CRuby's rb_f_p prints per argument.
    let mut rendered = Ok(());
    for a in args {
        match a.try_inspect_string() {
            Ok(s) => {
                buf.push_str(&s);
                buf.push('\n');
            }
            Err(sig) => {
                rendered = Err(sig);
                break;
            }
        }
    }
    if !buf.is_empty() {
        crate::builtins::io::write_str(&crate::builtins::io::current_stdout(), &buf)?;
    }
    rendered?;
    // `p one` answers the one value, `p a, b` the Array -- `pack`'s rule.
    Ok(crate::builtins::enumerable::pack(args))
}

/// `Kernel#pp` -- for this runtime's value shapes, `p`'s rendering.
pub fn kernel_pp(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    kernel_p(args)
}

/// The raw bytes of an output-separator global (`$,`, `$\`), or `None` when it
/// is nil -- which is its default and the overwhelmingly common case, so the
/// caller adds nothing at all rather than an empty slice.
/// `$,` (the output FIELD separator) or `$\` (the output RECORD
/// separator) as raw bytes, or `None` when unset -- which is the default
/// for both. Ruby resolves them at the CALL, never at stream creation, so
/// every reader asks here.
pub(crate) fn output_separator(name: &str) -> Option<crate::enc::StrBuf> {
    match crate::globals::global_get(0, name) {
        RubyValue::Str(s) => Some(s.lock().clone()),
        _ => None,
    }
}

/// `Kernel#print`: display renderings joined by the output field separator
/// `$,` and closed by the output record separator `$\`, both nil (so both
/// empty) unless the program sets them. A String argument contributes its RAW
/// bytes (see `io::display_bytes`), which is what keeps `print 0xB4.chr` a
/// single byte on the fd.
pub fn kernel_print(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    let mut buf = Vec::new();
    let mut rendered = Ok(());
    let field_sep = output_separator("$,");
    for (i, a) in args.iter().enumerate() {
        if i > 0
            && let Some(s) = &field_sep
        {
            buf.extend_from_slice(s.bytes());
        }
        if let Err(sig) = crate::builtins::io::display_bytes(a, &mut buf) {
            rendered = Err(sig);
            break;
        }
    }
    if rendered.is_ok()
        && let Some(s) = output_separator("$\\")
    {
        buf.extend_from_slice(s.bytes());
    }
    // Flush-then-propagate, same as `kernel_puts`.
    crate::builtins::io::write_bytes(&crate::builtins::io::current_stdout(), &buf)?;
    rendered?;
    Ok(RubyValue::Nil)
}

/// `Kernel#format`/`sprintf`.
pub fn kernel_format(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    let Some((RubyValue::Str(template), rest)) = args.split_first() else {
        return Err(type_error!("no format string given"));
    };
    crate::builtins::format::sprintf_encoded(template, rest)
}

/// `Kernel#printf`.
pub fn kernel_printf(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    if args.is_empty() {
        return Ok(RubyValue::Nil);
    }
    let formatted = kernel_format(args)?;
    crate::builtins::io::write_str(
        &crate::builtins::io::current_stdout(),
        &formatted.to_display_string(),
    )?;
    Ok(RubyValue::Nil)
}
