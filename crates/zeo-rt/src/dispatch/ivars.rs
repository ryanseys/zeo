//! The universal `Object#instance_variable_*` reflection helpers and the
//! `:@name` argument coercion they share.

use super::*;

/// The bare ivar name (`x`) from a `:@x`/`"@x"` reflection argument. A
/// non-symbol/string is a `TypeError`; anything that is not a valid ivar
/// name is a `NameError` -- both mirroring CRuby's own messages. Shared by
/// the universal `Object#instance_variable_*` helpers below (the
/// class-object table in `builtins::class_module` keeps its own parallel
/// copy).
///
/// The WHOLE name is checked, not just the leading `@`: `"@@bad"` is the
/// class-variable spelling and `"@1x"` starts with a digit, and both used to
/// pass here and answer nil for a get. The message quotes the raw name with
/// its `@`, which is what ruby prints.
pub fn ivar_name_arg(v: &RubyValue) -> Result<String, Signal> {
    // A symbol's text is already interned and `'static`, so the strip happens
    // before any allocation: `instance_variable_get(:@x)`, which is how this
    // is almost always spelled, allocated the name TWICE (once to own the
    // symbol's text, once for the `@`-stripped tail).
    if let RubyValue::Symbol(s) = v {
        let raw = s.name_str();
        return match crate::dispatch::names::is_ivar_name(raw) {
            true => Ok(raw[1..].to_string()),
            false => Err(name_error!(
                "'{raw}' is not allowed as an instance variable name"
            )),
        };
    }
    let raw = match v {
        RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
        _ => {
            return Err(type_error!(
                "{} is not a symbol nor a string",
                v.inspect_string()
            ));
        }
    };
    match crate::dispatch::names::is_ivar_name(&raw) {
        true => Ok(raw[1..].to_string()),
        false => Err(name_error!(
            "'{raw}' is not allowed as an instance variable name"
        )),
    }
}

/// `Object#instance_variable_get(:@x)` over ANY receiver: an `Object`'s named
/// ivar, a class object's own ivar (`civars`), or `nil` for a builtin/
/// immediate (which expose no Ruby-visible ivars in this runtime).
pub fn instance_variable_get(recv: &RubyValue, name_arg: &RubyValue) -> Result<RubyValue, Signal> {
    let name = ivar_name_arg(name_arg)?;
    crate::ractor::ivar_isolation_check(recv)?;
    Ok(match recv {
        RubyValue::Object(o) => o
            .ivar_get_named(&name)
            .or_else(|| crate::value_ivars::get(recv, &name))
            .unwrap_or(RubyValue::Nil),
        RubyValue::Class(cid) => crate::civars::class_ivar_get(cid.0, &name),
        _ => crate::value_ivars::get(recv, &name).unwrap_or(RubyValue::Nil),
    })
}

/// `Object#instance_variable_set(:@x, v)` -- writes the named ivar (of an
/// object or a class object), answering the value. A no-op on a
/// non-Object, non-Class receiver with no slot for the name. Carries
/// CRuby's `rb_check_frozen` (a frozen receiver raises before writing --
/// the same guard `ivar_set_dyn` and the static write path emit).
pub fn instance_variable_set(
    recv: &RubyValue,
    name_arg: &RubyValue,
    v: RubyValue,
) -> Result<RubyValue, Signal> {
    let name = ivar_name_arg(name_arg)?;
    crate::ractor::ivar_isolation_check(recv)?;
    match recv {
        RubyValue::Object(o) => {
            crate::builtins::check_frozen(recv)?;
            if !o.ivar_set_named(&name, v.clone()) {
                crate::value_ivars::set(recv, &name, v.clone());
            }
        }
        RubyValue::Class(cid) => crate::civars::class_ivar_set(cid.0, &name, v.clone())?,
        // A frozen builtin (immediates always; a frozen Str/Array/Hash)
        // raises like CRuby; an unfrozen one gets the identity-keyed store.
        other => {
            crate::builtins::check_frozen(other)?;
            crate::value_ivars::set(other, &name, v.clone());
        }
    }
    Ok(v)
}

/// `Object#remove_instance_variable(:@x)` -- removes the named ivar and
/// answers its former value, raising `NameError` if the object has no such
/// ivar. A frozen object raises `FrozenError` first (CRuby's order).
pub fn remove_instance_variable(
    recv: &RubyValue,
    name_arg: &RubyValue,
) -> Result<RubyValue, Signal> {
    let name = ivar_name_arg(name_arg)?;
    match recv {
        RubyValue::Object(o) => {
            crate::builtins::check_frozen(recv)?;
            match o
                .ivar_remove_named(&name)
                .or_else(|| crate::value_ivars::remove(recv, &name))
            {
                Some(v) => Ok(v),
                None => Err(name_error!("instance variable @{name} not defined")),
            }
        }
        // A class or module keeps its ivars in `civars`, not in `value_ivars`,
        // so this arm has to ask there. Without it `instance_variables` listed
        // a name that `remove_instance_variable` then called undefined --
        // `Bundler::Plugin.reset!` runs exactly that pair, one line apart.
        RubyValue::Class(cid) => crate::civars::class_ivar_remove(cid.0, &name)?
            .ok_or_else(|| name_error!("instance variable @{name} not defined")),
        _ => {
            crate::builtins::check_frozen(recv)?;
            crate::value_ivars::remove(recv, &name)
                .ok_or_else(|| name_error!("instance variable @{name} not defined"))
        }
    }
}

/// Whether `recv` has instance variable `@{bare_name}` actually set -- backs
/// `defined?(@iv)`, which answers `"instance-variable"` only for an assigned
/// ivar and `nil` otherwise. `bare_name` is the name without the leading `@`.
pub fn ivar_defined(recv: &RubyValue, bare_name: &str) -> bool {
    let want = Symbol::intern(&format!("@{bare_name}"));
    let RubyValue::Array(vars) = instance_variables(recv) else {
        return false;
    };

    vars.lock()
        .iter()
        .any(|v| matches!(v, RubyValue::Symbol(s) if *s == want))
}

/// `Object#instance_variables` -- the receiver's ivar names as `:@name`
/// symbols in declaration order (empty for a builtin/immediate).
pub fn instance_variables(recv: &RubyValue) -> RubyValue {
    let names: Vec<RubyValue> = match recv {
        // `ivar_pairs` already yields `@`-prefixed names (it backs the
        // default `Object#inspect`); `class_ivar_names` yields bare ones.
        // `value_ivars` contributes only for a hand-written runtime object,
        // whose `ivar_pairs` is empty -- see `ivar_set_dyn`'s Object arm.
        RubyValue::Object(o) => o
            .ivar_pairs()
            .into_iter()
            .map(|(n, _)| RubyValue::Symbol(Symbol::intern(&n)))
            .chain(
                crate::value_ivars::names(recv)
                    .into_iter()
                    .map(|n| RubyValue::Symbol(Symbol::intern(&format!("@{n}")))),
            )
            .collect(),
        RubyValue::Class(cid) => crate::civars::class_ivar_names(cid.0)
            .into_iter()
            .map(|n| RubyValue::Symbol(Symbol::intern(&format!("@{n}"))))
            .collect(),
        _ => crate::value_ivars::names(recv)
            .into_iter()
            .map(|n| RubyValue::Symbol(Symbol::intern(&format!("@{n}"))))
            .collect(),
    };
    RubyValue::Array(crate::array_new(names))
}
