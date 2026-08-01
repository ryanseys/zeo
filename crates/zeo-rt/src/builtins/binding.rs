//! `Binding` -- a compiled frame's scope, made into a value.
//!
//! A `Binding` is the one place an AOT compiler has to give ground: every
//! other Ruby local lives as a Rust stack slot, but a scope that calls
//! `binding` hands its locals out to code that isn't compiled yet, so those
//! slots must be addressable by NAME and shared by REFERENCE. The compiler
//! answers exactly that and no more (`codegen::captures::binding_scope_names`):
//! a scope containing a `binding` call gives EVERY one of its own locals the
//! `Arc<Mutex<RubyValue>>` cell storage class an escaping block's captures
//! already use, then hands the `(name, cell)` list to [`binding_new`]. Nothing
//! else in the program pays for it.
//!
//! Because the cells are shared, not snapshotted, mutation flows both ways --
//! `b.local_variable_set(:x, 5)` is visible to the compiled scope, and a later
//! `x = 6` there is visible through `b`, exactly as CRuby's frame-sharing
//! `Binding` behaves.
//!
//! [`BindingScope`] is two-tiered because CRuby's is: `dup` copies the Binding
//! but not its environment (writes through the copy reach the original's
//! locals), while a local ADDED to the copy stays on the copy. The `frame`
//! tier is the shared original; `added` is this Binding's own, newest-first --
//! which is also the order `local_variables` reports them in.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;
use zeo_abi::ClassId;
use zeo_macros::ruby_class;

use crate::builtins::{name_error, type_error};
use crate::dispatch::{RObj, RubyObject};
use crate::signal::Signal;
use crate::symbol::Symbol;
use crate::value::RubyValue;

/// One local variable's storage -- the very cell the compiled scope reads
/// and writes (see `codegen::hoisting::LocalStorage::Captured`).
pub type LocalCell = Arc<Mutex<RubyValue>>;

/// The variable environment behind a `Binding` -- see the module docs for
/// the two-tier split.
pub struct BindingScope {
    /// Locals this Binding added after capture, newest first.
    added: Mutex<Vec<(String, LocalCell)>>,
    /// The captured frame's own slots, in declaration order, shared with
    /// every `dup` and with the compiled scope itself.
    frame: Arc<Vec<(String, LocalCell)>>,
}

impl BindingScope {
    pub(crate) fn new(frame: Vec<(String, LocalCell)>) -> BindingScope {
        BindingScope {
            added: Mutex::new(Vec::new()),
            frame: Arc::new(frame),
        }
    }

    fn cell(&self, name: &str) -> Option<LocalCell> {
        if let Some((_, c)) = self.added.lock().iter().find(|(n, _)| n == name) {
            return Some(Arc::clone(c));
        }
        self.frame
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, c)| Arc::clone(c))
    }

    pub(crate) fn get(&self, name: &str) -> Option<RubyValue> {
        self.cell(name).map(|c| c.lock().clone())
    }

    pub(crate) fn defined(&self, name: &str) -> bool {
        self.cell(name).is_some()
    }

    /// Write `name`, adding it to this Binding alone when the captured frame
    /// has no such slot (CRuby grows the Binding's own env, and a compiled
    /// scope has no room to grow).
    pub(crate) fn set(&self, name: &str, value: RubyValue) {
        if let Some(cell) = self.cell(name) {
            *cell.lock() = value;
            return;
        }
        self.added
            .lock()
            .insert(0, (name.to_string(), Arc::new(Mutex::new(value))));
    }

    /// Every name in scope, `local_variables` order: this Binding's own
    /// additions newest-first, then the frame's own declaration order.
    fn names(&self) -> Vec<String> {
        let mut out: Vec<String> = self.added.lock().iter().map(|(n, _)| n.clone()).collect();
        out.extend(self.frame.iter().map(|(n, _)| n.clone()));
        out
    }

    /// A CHILD of this scope: the same cells, so a write to a local that
    /// already exists still reaches the frame, but with its own layer for
    /// names the code introduces. That layer is CRuby's bare `eval`, whose new
    /// locals die with the call -- unlike `Binding#eval`, which runs in the
    /// Binding itself and keeps them.
    pub(crate) fn child(&self) -> Arc<BindingScope> {
        Arc::new(BindingScope {
            added: Mutex::new(Vec::new()),
            frame: self.dup_frame(),
        })
    }

    /// The frame a `dup` inherits: this scope's whole current contents, cells
    /// and all, so a write through the copy still reaches the original --
    /// while the copy's `added` starts empty.
    fn dup_frame(&self) -> Arc<Vec<(String, LocalCell)>> {
        let added = self.added.lock();
        if added.is_empty() {
            return Arc::clone(&self.frame);
        }
        let mut merged: Vec<(String, LocalCell)> = added.clone();
        merged.extend(self.frame.iter().cloned());
        Arc::new(merged)
    }
}

pub struct RBinding {
    /// What `self` means inside this scope -- `Binding#receiver`, and the
    /// receiver an implicit-self call in `Binding#eval` dispatches on.
    pub self_val: RubyValue,
    pub scope: Arc<BindingScope>,
    /// `Binding#source_location` -- where the `binding` call itself sits.
    pub file: String,
    pub line: u32,
    /// The defining box, for constant/global resolution in `#eval`.
    pub box_id: u32,
    /// The lexical class enclosing the capture -- CRuby's cref, which is what
    /// makes `binding.eval("K")` inside `module M` find `M::K`. `None` at the
    /// top level.
    pub cref: Option<ClassId>,
    pub(crate) frozen: AtomicBool,
}

impl RubyObject for RBinding {
    fn class_id(&self) -> ClassId {
        zeo_abi::BINDING_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        self.frozen.load(Ordering::Relaxed)
    }
    fn set_frozen(&self) {
        self.frozen.store(true, Ordering::Relaxed)
    }
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, copy_frozen: bool) -> RObj {
        let d = RBinding {
            self_val: self.self_val.clone(),
            scope: Arc::new(BindingScope {
                added: Mutex::new(Vec::new()),
                frame: self.scope.dup_frame(),
            }),
            file: self.file.clone(),
            line: self.line,
            box_id: self.box_id,
            cref: self.cref,
            frozen: AtomicBool::new(false),
        };
        if copy_frozen {
            d.set_frozen();
        }
        Arc::new(d)
    }
}

/// The compiled scope's own `binding` call: `locals` is that scope's every
/// local, in `local_variables` order, each one the live cell the generated
/// code keeps reading and writing.
pub fn binding_new(
    self_val: RubyValue,
    locals: Vec<(&'static str, LocalCell)>,
    file: &'static str,
    line: u32,
    box_id: u32,
    cref: u32,
) -> RubyValue {
    binding_value(
        self_val,
        Arc::new(BindingScope::new(
            locals
                .into_iter()
                .map(|(n, c)| (n.to_string(), c))
                .collect(),
        )),
        file.to_string(),
        line,
        box_id,
        // `u32::MAX` is codegen's "no enclosing class" marker: `ClassId(0)` is
        // `Object`, a real cref a top-level binding must NOT claim.
        (cref != u32::MAX).then_some(ClassId(cref)),
    )
}

pub(crate) fn binding_value(
    self_val: RubyValue,
    scope: Arc<BindingScope>,
    file: String,
    line: u32,
    box_id: u32,
    cref: Option<ClassId>,
) -> RubyValue {
    RubyValue::Object(Arc::new(RBinding {
        self_val,
        scope,
        file,
        line,
        box_id,
        cref,
        frozen: AtomicBool::new(false),
    }))
}

/// A new Binding object over the SAME environment -- what `Proc#binding`
/// answers, since CRuby hands back a distinct object each call while the two
/// still name one scope. A non-Binding argument can't arise (the only caller
/// passes what codegen stored) and is returned unchanged.
pub(crate) fn rebind(v: &RubyValue) -> RubyValue {
    let Some(b) = as_binding(v) else {
        return v.clone();
    };
    binding_value(
        b.self_val.clone(),
        Arc::clone(&b.scope),
        b.file.clone(),
        b.line,
        b.box_id,
        b.cref,
    )
}

pub(crate) fn as_binding(v: &RubyValue) -> Option<&RBinding> {
    match v {
        RubyValue::Object(o) => o.as_any().downcast_ref::<RBinding>(),
        _ => None,
    }
}

fn recv_binding(recv: &RubyValue) -> &RBinding {
    as_binding(recv).expect("the Binding table only dispatches on Binding receivers")
}

/// A `Symbol`-or-`String` variable name, CRuby's coercion for every
/// `local_variable_*` argument.
fn var_name(v: &RubyValue) -> Result<String, Signal> {
    match v {
        RubyValue::Symbol(s) => Ok(s.name().to_string()),
        RubyValue::Str(s) => Ok(s.lock().to_utf8_lossy().into_owned()),
        other => Err(type_error!(
            "{} is not a symbol nor a string",
            other.inspect_string()
        )),
    }
}

fn inspect_of(recv: &RubyValue) -> String {
    let addr = match recv {
        RubyValue::Object(o) => Arc::as_ptr(o) as *const () as usize,
        _ => 0,
    };
    format!("#<Binding:0x{addr:016x}>")
}

ruby_class! {
    Binding = zeo_abi::BINDING_CLASS < zeo_abi::OBJECT_CLASS;

    def "receiver"(recv) {
        Ok(recv_binding(recv).self_val.clone())
    }
    def "source_location"(recv) {
        let b = recv_binding(recv);
        Ok(RubyValue::Array(crate::array_new(vec![
            RubyValue::Str(crate::string_new(b.file.clone())),
            RubyValue::Int(b.line as i64),
        ])))
    }
    def "local_variables"(recv) {
        let names = recv_binding(recv).scope.names();
        Ok(RubyValue::Array(crate::array_new(
            names.iter().map(|n| RubyValue::Symbol(Symbol::intern(n))).collect(),
        )))
    }
    // The implicit block parameters (`it`, `_1`..`_9`). zeo compiles them to
    // ordinary block parameters, so a Binding carries no separate implicit
    // set -- which is the empty answer CRuby gives for every binding taken
    // outside such a block.
    def "implicit_parameters"(_recv) {
        Ok(RubyValue::Array(crate::array_new(Vec::new())))
    }
    def "implicit_parameter_defined?"(_recv, _name) {
        Ok(RubyValue::Bool(false))
    }
    def "implicit_parameter_get"(recv, name) {
        Err(crate::builtins::name_error!(
            "implicit parameter '{}' is not defined for {}",
            var_name(name)?,
            recv.inspect_string()
        ))
    }
    // `Binding#irb` opens an IRB session on this scope. zeo ships no irb, and
    // a `require` for it is the LoadError a caller can rescue.
    def "irb" cfunc (_recv, *_args, &_block) {
        Err(crate::dispatch::raise_error(
            "LoadError",
            "cannot load such file -- irb".to_string(),
        ))
    }
    def "local_variable_defined?"(recv, arg) {
        Ok(RubyValue::Bool(recv_binding(recv).scope.defined(&var_name(arg)?)))
    }
    def "local_variable_get"(recv, arg) {
        let name = var_name(arg)?;
        recv_binding(recv).scope.get(&name).ok_or_else(|| {
            name_error!("local variable '{name}' is not defined for {}", inspect_of(recv))
        })
    }
    def "local_variable_set"(recv, arg1, arg2) {
        let name = var_name(arg1)?;
        recv_binding(recv).scope.set(&name, (*arg2).clone());
        Ok((*arg2).clone())
    }
    // `eval(src, file = "(eval)", line = 1)` -- the source runs in THIS
    // scope: its locals, its `self`, its lexical constants.
    def "eval" arity -1 (recv, arg1, arg2?, arg3?) {
        let file = match arg2 {
            Some(v) => Some(crate::builtins::convert::to_rstr(v)?.lock().to_utf8_lossy().into_owned()),
            None => None,
        };
        let line = match arg3 {
            Some(v) => Some(crate::builtins::convert::to_index(v)? as u32),
            None => None,
        };
        crate::eval_vm::eval_with_binding(arg1, recv_binding(recv), file, line)
    }
    def "inspect" | "to_s"(recv) {
        Ok(RubyValue::Str(crate::string_new(inspect_of(recv))))
    }
}
