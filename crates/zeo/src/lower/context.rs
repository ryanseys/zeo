//! The lowering CONTEXT: which file is being lowered, and which locals are
//! bound to `Ruby::Box` handles there. This is the one seam through which
//! the loader (the require-resolving driver) feeds state INTO `lower_node`'s
//! recursion -- the recognizers for `__FILE__`/`__dir__` and `box.eval`/
//! `box::X` read it, and the loader pushes/pops it as it splices files.
//!
//! Thread-locals rather than threaded parameters because `lower_node`'s
//! recursion would otherwise need a context argument through every
//! recognizer; lowering is single-threaded, and both stacks are empty
//! outside a loader-driven lowering (a bare `parse_and_lower_into` -- the
//! exception prelude, `eval` bodies, tests -- sees `None` from every
//! reader, which is exactly the no-file/no-boxes answer it needs).

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// Per-FILE `box = Ruby::Box.new` handle bindings, as a stack --
// one frame per file currently being lowered (recognition happens during
// that file's own lowering, BEFORE its rename pass, so original local
// names are the right key; frames never leak across files).
thread_local! {
    static BOX_BINDINGS: RefCell<Vec<HashMap<String, u32>>> = const { RefCell::new(Vec::new()) };
    /// The path of the file currently being lowered -- a stack for the same
    /// reason `BOX_BINDINGS` is one: `require` splices a child file's
    /// statements into the parent's list mid-walk, so the "current file"
    /// has to restore when that splice finishes. Consulted by
    /// the `__FILE__`/`__LINE__`/`__dir__` recognizers.
    ///
    /// This is what makes `__FILE__` name the file the code was WRITTEN in
    /// rather than the main program: every file's statements end up in one
    /// merged `Program`, so by codegen time there is nothing left to tell
    /// them apart.
    static SOURCE_FILE: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
}

/// The box bound to local `name` in the file currently being lowered, if
/// any -- consulted by the `box::X`/`box.eval` recognizers.
pub fn current_box_binding(name: &str) -> Option<u32> {
    BOX_BINDINGS.with(|b| b.borrow().last().and_then(|m| m.get(name).copied()))
}

/// The file currently being lowered -- `None` when compiling a source
/// string with no path at all (`compile_to_rust`'s bare form, and the
/// exception prelude), where real Ruby's own answer would be `"-e"`.
pub fn current_source_file() -> Option<PathBuf> {
    SOURCE_FILE.with(|f| f.borrow().last().cloned())
}

/// Pushes the file being lowered; pops on drop (including the error path).
/// Same RAII shape as `BindingsFrame`.
pub struct SourceFileFrame;
impl SourceFileFrame {
    pub fn push(path: Option<&Path>) -> Option<SourceFileFrame> {
        let path = path?;
        SOURCE_FILE.with(|f| f.borrow_mut().push(path.to_path_buf()));
        Some(SourceFileFrame)
    }
}
impl Drop for SourceFileFrame {
    fn drop(&mut self) {
        SOURCE_FILE.with(|f| {
            f.borrow_mut().pop();
        });
    }
}

/// Pushes a fresh bindings frame for one file's lowering; pops on drop
/// (including the error path).
pub struct BindingsFrame;
impl BindingsFrame {
    pub fn push() -> BindingsFrame {
        BOX_BINDINGS.with(|b| b.borrow_mut().push(HashMap::new()));
        BindingsFrame
    }
    pub fn bind(&self, name: String, box_id: u32) {
        BOX_BINDINGS.with(|b| {
            b.borrow_mut()
                .last_mut()
                .expect("frame pushed")
                .insert(name, box_id);
        });
    }
}
impl Drop for BindingsFrame {
    fn drop(&mut self) {
        BOX_BINDINGS.with(|b| {
            b.borrow_mut().pop();
        });
    }
}
