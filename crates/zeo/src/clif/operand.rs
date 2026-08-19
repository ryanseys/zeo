//! The expression currency: a lowered Ruby value, either unboxed SSA (tag
//! statically known) or a 24-byte value in memory with an ownership bit.

use cranelift_codegen::ir;
use zeo_abi::abi::ValueTag;

/// What the emitter statically knows about a boxed value's tag.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum TagInfo {
    Known(u8),
    Unknown,
}

impl TagInfo {
    /// Statically heap (needs retain/release/pooling)?  `None` = unknown,
    /// decide at run time.
    pub fn heap(self) -> Option<bool> {
        match self {
            TagInfo::Known(t) => Some(t >= zeo_abi::abi::FIRST_HEAP_TAG),
            TagInfo::Unknown => None,
        }
    }
}

/// One lowered expression's value.
pub(crate) enum Operand {
    /// `nil` -- no payload.
    Nil,
    /// An unboxed `i64` (tag `Int`).
    Int(ir::Value),
    /// An unboxed `f64` (tag `Float`).
    Float(ir::Value),
    /// An unboxed `i8` 0/1 (tag `Bool`).
    Bool(ir::Value),
    /// A 24-byte value in a stack slot. `owned` = this operand is the
    /// value's owner and must move, pool, or release it exactly once.
    Slot {
        ss: ir::StackSlot,
        owned: bool,
        tag: TagInfo,
    },
    /// A 24-byte value behind a pointer (a local slot, a param).
    Ptr {
        addr: ir::Value,
        owned: bool,
        tag: TagInfo,
    },
}

impl Operand {
    pub fn tag(&self) -> TagInfo {
        match self {
            Operand::Nil => TagInfo::Known(ValueTag::Nil as u8),
            Operand::Int(_) => TagInfo::Known(ValueTag::Int as u8),
            Operand::Float(_) => TagInfo::Known(ValueTag::Float as u8),
            Operand::Bool(_) => TagInfo::Known(ValueTag::Bool as u8),
            Operand::Slot { tag, .. } | Operand::Ptr { tag, .. } => *tag,
        }
    }

    pub fn owned(&self) -> bool {
        match self {
            Operand::Nil | Operand::Int(_) | Operand::Float(_) | Operand::Bool(_) => false,
            Operand::Slot { owned, .. } | Operand::Ptr { owned, .. } => *owned,
        }
    }
}
