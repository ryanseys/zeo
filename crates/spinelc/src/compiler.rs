//! `spinelc`'s own analog of spinel's `Compiler`/`ClassInfo`/`Scope`
//! (`compiler.h`) -- pure compile-time bookkeeping. It never appears in the
//! generated program; codegen consults it and discards it. See the plan's
//! "Two `ClassId` types, on purpose": this `ClassId` is numerically mirrored
//! into `spinel_rt::ClassId` by codegen, but the two types are otherwise
//! unrelated -- `spinelc` never links against `spinel-rt` at all.

use crate::hir::{Hir, NodeId, Params};
use crate::types::TyKind;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ClassId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ScopeId(pub u32);

/// `ClassId(0)`, always present, no ivars, no methods, no superclass -- the
/// root every user class ultimately chains up to. Mirrors spinel reserving
/// class index 0 as the implicit root.
pub const OBJECT_CLASS: ClassId = ClassId(0);

pub struct ClassInfo {
    pub name: String,
    pub parent: Option<ClassId>,
    pub ivars: Vec<String>,
    /// This class's OWN scopes only (not inherited) -- `method_in_chain`
    /// walks `.parent` to search ancestors, exactly like spinel's
    /// `comp_method_in_chain` (compiler.c:404).
    pub methods: Vec<ScopeId>,
}

pub struct Scope {
    pub name: String,
    pub class: Option<ClassId>,
    pub params: Params,
    pub body: Vec<NodeId>,
    /// Per-local static type, computed once by `analyze::locals::infer_locals`
    /// (a forward, single-pass walk -- not the deferred whole-program
    /// fixpoint). Lets codegen resolve `x + y` to native `Int` arithmetic
    /// when `x`/`y` are locals, not just literal-on-literal operands.
    pub local_types: HashMap<String, TyKind>,
}

pub struct Compiler {
    pub hir: Hir,
    pub classes: Vec<ClassInfo>,
    pub scopes: Vec<Scope>,
}

impl Compiler {
    pub fn new(hir: Hir) -> Compiler {
        Compiler {
            hir,
            classes: vec![ClassInfo {
                name: "Object".to_string(),
                parent: None,
                ivars: Vec::new(),
                methods: Vec::new(),
            }],
            scopes: Vec::new(),
        }
    }

    pub fn class_by_name(&self, name: &str) -> Option<ClassId> {
        self.classes
            .iter()
            .position(|c| c.name == name)
            .map(|i| ClassId(i as u32))
    }

    pub fn add_class(&mut self, name: String, parent: ClassId) -> ClassId {
        self.classes.push(ClassInfo {
            name,
            parent: Some(parent),
            ivars: Vec::new(),
            methods: Vec::new(),
        });
        ClassId((self.classes.len() - 1) as u32)
    }

    pub fn add_scope(&mut self, scope: Scope) -> ScopeId {
        let id = ScopeId(self.scopes.len() as u32);
        if let Some(class) = scope.class {
            self.scopes.push(scope);
            self.classes[class.0 as usize].methods.push(id);
        } else {
            self.scopes.push(scope);
        }
        id
    }

    pub fn scope(&self, id: ScopeId) -> &Scope {
        &self.scopes[id.0 as usize]
    }

    pub fn class(&self, id: ClassId) -> &ClassInfo {
        &self.classes[id.0 as usize]
    }

    /// Mirrors `comp_method_in_chain` (compiler.c:404-411) exactly,
    /// including the walk direction: start at the receiver's class, walk
    /// `.parent` until a class defines the method or the chain runs out.
    pub fn method_in_chain(&self, mut class: ClassId, name: &str) -> Option<(ClassId, ScopeId)> {
        loop {
            let info = &self.classes[class.0 as usize];
            if let Some(&sid) = info
                .methods
                .iter()
                .find(|&&s| self.scopes[s.0 as usize].name == name)
            {
                return Some((class, sid));
            }
            class = info.parent?;
        }
    }
}
