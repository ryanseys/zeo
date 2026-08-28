//! `Readline::HISTORY` -- the process's ONE line-editing history as an
//! Enumerable object. Every row reads the shared [`super::STATE`] list, the
//! same list `Readline.readline(prompt, true)` appends to; the object itself
//! is a stateless singleton handle (CRuby's is an `Object` extended with a
//! module, equally payload-free).

use crate::builtins::convert;
use crate::dispatch::{RObj, RubyObject};
use crate::{RubyValue, Signal};
use std::sync::Arc;
use zeo_macros::ruby_class;

struct RHistory;

impl RubyObject for RHistory {
    fn class_id(&self) -> crate::ClassId {
        zeo_abi::READLINE_HISTORY_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    // A process-wide state holder, like the std streams: freezing it is
    // meaningless.
    fn is_frozen(&self) -> bool {
        false
    }
    fn set_frozen(&self) {}
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, _copy_frozen: bool) -> RObj {
        Arc::new(RHistory)
    }
}

/// The `Readline::HISTORY` singleton.
pub(super) fn history_value() -> RubyValue {
    static V: std::sync::LazyLock<RubyValue> =
        std::sync::LazyLock::new(|| RubyValue::Object(Arc::new(RHistory)));
    V.clone()
}

/// A (possibly negative) Ruby index into the current list, or CRuby's
/// `IndexError` -- the classic extension's message, index included.
fn resolve(i: i64, len: usize) -> Result<usize, Signal> {
    let idx = if i < 0 { i + len as i64 } else { i };
    if idx < 0 || idx as usize >= len {
        return Err(crate::builtins::index_error!("invalid index ({i})"));
    }
    Ok(idx as usize)
}

fn str_value(s: &str) -> RubyValue {
    RubyValue::Str(crate::string_new(s.to_string()))
}

fn push_arg(v: &RubyValue) -> Result<String, Signal> {
    Ok(convert::to_rstr(v)?.lock().to_utf8_lossy().into_owned())
}

ruby_class! {
    History = zeo_abi::READLINE_HISTORY_CLASS < zeo_abi::OBJECT_CLASS;
    include zeo_abi::ENUMERABLE_CLASS;

    def "push" (recv, *args, &_block) {
        let mut st = super::STATE.lock();
        for v in args {
            let s = push_arg(v)?;
            st.history.push(s);
        }
        Ok(recv.clone())
    }
    def "<<" (recv, other) {
        super::STATE.lock().history.push(push_arg(other)?);
        Ok(recv.clone())
    }

    def "[]" (_recv, arg) {
        let st = super::STATE.lock();
        let i = resolve(convert::to_index(arg)?, st.history.len())?;
        Ok(str_value(&st.history[i]))
    }
    def "[]=" (_recv, arg1, arg2) {
        let s = push_arg(arg2)?;
        let mut st = super::STATE.lock();
        let i = resolve(convert::to_index(arg1)?, st.history.len())?;
        st.history[i] = s;
        Ok((*arg2).clone())
    }

    def "delete_at" (_recv, arg) {
        let mut st = super::STATE.lock();
        let i = resolve(convert::to_index(arg)?, st.history.len())?;
        Ok(str_value(&st.history.remove(i)))
    }
    def "pop" cfunc (_recv) {
        Ok(match super::STATE.lock().history.pop() {
            Some(s) => str_value(&s),
            None => RubyValue::Nil,
        })
    }
    def "shift" cfunc (_recv) {
        let mut st = super::STATE.lock();
        if st.history.is_empty() {
            return Ok(RubyValue::Nil);
        }
        Ok(str_value(&st.history.remove(0)))
    }

    // `each` powers the whole included-Enumerable surface (`to_a`,
    // `include?`, ...). The snapshot up front keeps the lock out of the
    // block, which may itself touch HISTORY.
    def "each" (recv, &block) {
        let p = crate::builtins::need_block!(block);
        let snapshot = super::STATE.lock().history.clone();
        for line in &snapshot {
            p.call(&[str_value(line)])?;
        }
        Ok(recv.clone())
    }

    def "length" | "size" (_recv) {
        Ok(RubyValue::Int(super::STATE.lock().history.len() as i64))
    }
    def "empty?" (_recv) {
        Ok(RubyValue::Bool(super::STATE.lock().history.is_empty()))
    }
    def "clear" (recv) {
        super::STATE.lock().history.clear();
        Ok(recv.clone())
    }

    // The classic extension's own answer.
    def "to_s" (_recv) {
        Ok(str_value("HISTORY"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_registered_and_rows_share_the_one_list() {
        let table = crate::builtins::registered_table(zeo_abi::READLINE_HISTORY_CLASS)
            .expect("Readline::HISTORY is a registered builtin table");
        let lookup = table
            .instance
            .as_ref()
            .expect("HISTORY has instance rows")
            .lookup;
        let h = history_value();

        let push = lookup("push").expect("push row");
        push(&h, &[str_value("one"), str_value("two")], None).unwrap();
        let len = lookup("length").expect("length row");
        assert!(matches!(len(&h, &[], None).unwrap(), RubyValue::Int(n) if n >= 2));

        let at = lookup("[]").expect("[] row");
        assert!(
            matches!(at(&h, &[RubyValue::Int(-1)], None).unwrap(), RubyValue::Str(s) if s.lock().to_utf8_lossy() == "two")
        );

        let clear = lookup("clear").expect("clear row");
        clear(&h, &[], None).unwrap();
        let empty = lookup("empty?").expect("empty? row");
        assert!(matches!(
            empty(&h, &[], None).unwrap(),
            RubyValue::Bool(true)
        ));
    }
}
