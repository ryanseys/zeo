//! The `ffi` gem's scalar C-type surface, single-sourced.
//!
//! The compiler (keyword -> `FfiType` in `lower/ffi.rs`, widths in struct
//! layouts and `#[repr(C)]` mirrors) and the runtime (varargs marshaling,
//! `FFI::Type` objects, `MemoryPointer` element sizes) must agree on what
//! `:int` means and how wide it is -- a disagreement is silent memory
//! corruption, not an error. Both sides read THIS table; neither keeps a
//! copy. LP64 throughout (`:long`/`:ulong` = 64 bits), matching macOS and
//! Linux, the targets zeo builds for.

/// One scalar C type as the `ffi` gem names it. This is the runtime's
/// `FfiKind` (re-exported there) and the scalar half of the compiler's
/// `FfiType`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CScalar {
    Void,
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    /// `:long`/`:ulong` (and the gem's `:size_t`/`:ssize_t` aliases): the
    /// same eight bytes as `I64`/`U64` on LP64, kept apart because the gem
    /// converts them through `NUM2LONG`/`NUM2ULONG` rather than
    /// `NUM2LL`/`NUM2ULL`, whose out-of-range and type errors are worded
    /// differently, and because `FFI::Type::LONG` is not `INT64`.
    Long,
    ULong,
    F32,
    F64,
    /// A C `_Bool` -- one byte in memory.
    Bool,
    /// `:string` -- a `const char *` with copy-in/read-back marshaling.
    Str,
    /// `:pointer` -- an opaque `void *`.
    Pointer,
}

impl CScalar {
    /// The C type's byte width (LP64). `Void` is 0 -- it never occupies a
    /// field; libffi's own `ffi_type_void` reports 1, which the runtime's
    /// `FFI::Type::VOID` preserves as its local quirk.
    pub const fn size(self) -> usize {
        match self {
            CScalar::Void => 0,
            CScalar::I8 | CScalar::U8 | CScalar::Bool => 1,
            CScalar::I16 | CScalar::U16 => 2,
            CScalar::I32 | CScalar::U32 | CScalar::F32 => 4,
            CScalar::I64
            | CScalar::U64
            | CScalar::Long
            | CScalar::ULong
            | CScalar::F64
            | CScalar::Str
            | CScalar::Pointer => 8,
        }
    }

    /// The C ABI alignment -- equal to the width for every scalar here.
    pub const fn align(self) -> usize {
        self.size()
    }

    /// The variant as ONE BYTE, for a `.rodata` call descriptor the
    /// Cranelift backend bakes and the runtime reads back
    /// (`zeo_abi::abi::FfiTypeC`). Spelled out rather than left to a
    /// `repr` so a reordering of the enum cannot silently retype a call.
    pub const fn code(self) -> u8 {
        match self {
            CScalar::Void => 0,
            CScalar::I8 => 1,
            CScalar::I16 => 2,
            CScalar::I32 => 3,
            CScalar::I64 => 4,
            CScalar::U8 => 5,
            CScalar::U16 => 6,
            CScalar::U32 => 7,
            CScalar::U64 => 8,
            CScalar::F32 => 9,
            CScalar::F64 => 10,
            CScalar::Bool => 11,
            CScalar::Str => 12,
            CScalar::Pointer => 13,
            CScalar::Long => 14,
            CScalar::ULong => 15,
        }
    }

    /// The inverse of [`CScalar::code`].
    pub const fn from_code(code: u8) -> Option<CScalar> {
        Some(match code {
            0 => CScalar::Void,
            1 => CScalar::I8,
            2 => CScalar::I16,
            3 => CScalar::I32,
            4 => CScalar::I64,
            5 => CScalar::U8,
            6 => CScalar::U16,
            7 => CScalar::U32,
            8 => CScalar::U64,
            9 => CScalar::F32,
            10 => CScalar::F64,
            11 => CScalar::Bool,
            12 => CScalar::Str,
            13 => CScalar::Pointer,
            14 => CScalar::Long,
            15 => CScalar::ULong,
            _ => return None,
        })
    }

    /// The variant's own name (`"I32"`), which is also the runtime `FfiKind`
    /// variant name codegen spells in generated code.
    pub const fn name(self) -> &'static str {
        match self {
            CScalar::Void => "Void",
            CScalar::I8 => "I8",
            CScalar::I16 => "I16",
            CScalar::I32 => "I32",
            CScalar::I64 => "I64",
            CScalar::U8 => "U8",
            CScalar::U16 => "U16",
            CScalar::U32 => "U32",
            CScalar::U64 => "U64",
            CScalar::Long => "Long",
            CScalar::ULong => "ULong",
            CScalar::F32 => "F32",
            CScalar::F64 => "F64",
            CScalar::Bool => "Bool",
            CScalar::Str => "Str",
            CScalar::Pointer => "Pointer",
        }
    }

    /// The CANONICAL gem keyword for this scalar (`:int32`-style exact-width
    /// spellings) -- what synthesized Ruby source writes in a type position.
    /// Every one of these round-trips through `from_keyword`.
    pub const fn keyword(self) -> &'static str {
        match self {
            CScalar::Void => "void",
            CScalar::I8 => "int8",
            CScalar::I16 => "int16",
            CScalar::I32 => "int32",
            CScalar::I64 => "int64",
            CScalar::U8 => "uint8",
            CScalar::U16 => "uint16",
            CScalar::U32 => "uint32",
            CScalar::U64 => "uint64",
            CScalar::Long => "long",
            CScalar::ULong => "ulong",
            CScalar::F32 => "float",
            CScalar::F64 => "double",
            CScalar::Bool => "bool",
            CScalar::Str => "string",
            CScalar::Pointer => "pointer",
        }
    }

    /// A type KEYWORD as the gem spells it (`:int`, `:size_t`, `:buffer_in`).
    /// Covers the scalar surface plus the gem's aliases; `None` for a name
    /// only a `typedef`/`enum`/`callback` declaration (or `from_c_typedef`)
    /// can answer.
    pub fn from_keyword(name: &str) -> Option<CScalar> {
        Some(match name {
            "void" => CScalar::Void,
            "char" | "int8" => CScalar::I8,
            "short" | "int16" => CScalar::I16,
            "int" | "int32" => CScalar::I32,
            "long_long" | "int64" => CScalar::I64,
            "long" | "ssize_t" => CScalar::Long,
            "uchar" | "uint8" => CScalar::U8,
            "ushort" | "uint16" => CScalar::U16,
            "uint" | "uint32" => CScalar::U32,
            "ulong_long" | "uint64" => CScalar::U64,
            "ulong" | "size_t" => CScalar::ULong,
            "float" => CScalar::F32,
            "double" => CScalar::F64,
            "bool" => CScalar::Bool,
            "string" => CScalar::Str,
            "pointer" | "buffer_in" | "buffer_out" | "buffer_inout" => CScalar::Pointer,
            _ => return None,
        })
    }

    /// The C typedefs the real `ffi` gem resolves natively, restricted to
    /// those whose width and signedness are the SAME on every target zeo
    /// builds for.
    ///
    /// The gem's table is derived from the headers of the machine it runs
    /// on, so copying it wholesale would bake this machine's platform into
    /// the compiler -- and several POSIX typedefs genuinely differ between
    /// macOS and glibc: `mode_t` (uint16 vs uint32), `dev_t` (int32 vs
    /// uint64), `nlink_t` (uint16 vs uint64), `sa_family_t` (uint8 vs
    /// uint16), `blksize_t` and `suseconds_t` (int32 vs int64), `clock_t`
    /// (unsigned vs signed). Those stay a clean rejection: resolving one
    /// here would silently shift every field after it in a struct layout,
    /// which is a wrong answer rather than a missing one.
    ///
    /// What is left is safe by definition rather than by observation: the
    /// C99 exact-width names are exact everywhere, the pointer-width names
    /// follow the LP64 assumption `:long` already makes, and the handful of
    /// POSIX types below agree on both targets. Falls through to the Win32
    /// vocabulary, whose widths the Win32 API fixes on EVERY platform.
    pub fn from_c_typedef(name: &str) -> Option<CScalar> {
        // BSD's `u_int32_t` IS `uint32_t` -- rewrite the prefix so it lands
        // on the UNSIGNED row (a bare strip would land on the signed one).
        if let Some(rest) = name.strip_prefix("u_") {
            return CScalar::from_c_typedef(&format!("u{rest}"));
        }
        // `__int32_t` is the glibc-internal spelling of the same exact-width
        // type; gems reach for whichever their headers showed them.
        let bare = name.strip_prefix("__").unwrap_or(name);
        Some(match bare {
            "int8_t" | "int_least8_t" => CScalar::I8,
            "int16_t" | "int_least16_t" => CScalar::I16,
            "int32_t" | "int_least32_t" => CScalar::I32,
            "int64_t" | "int_least64_t" => CScalar::I64,
            "uint8_t" | "uint_least8_t" => CScalar::U8,
            "uint16_t" | "uint_least16_t" => CScalar::U16,
            "uint32_t" | "uint_least32_t" => CScalar::U32,
            "uint64_t" | "uint_least64_t" => CScalar::U64,
            // Pointer-width, on the LP64 assumption `:long` already makes.
            "intptr_t" | "ptrdiff_t" | "intmax_t" => CScalar::I64,
            "uintptr_t" | "uintmax_t" => CScalar::U64,
            // POSIX types that are the same on macOS and 64-bit glibc.
            // `off_t` is 64-bit on both (the gem builds with large-file
            // support); `time_t` is the 64-bit signed one on every 64-bit
            // target.
            "off_t" | "time_t" | "blkcnt_t" | "register_t" => CScalar::I64,
            "pid_t" | "key_t" => CScalar::I32,
            "uid_t" | "gid_t" | "id_t" | "socklen_t" | "in_addr_t" | "useconds_t" => CScalar::U32,
            "in_port_t" => CScalar::U16,
            "ino_t" | "rlim_t" => CScalar::U64,
            "caddr_t" => CScalar::Pointer,
            // NOT here on purpose: `int_fast16_t`/`int_fast32_t`, which are
            // 16/32 bits on macOS and 64 on glibc.
            _ => return CScalar::from_win32(name),
        })
    }

    /// The Win32 typedef vocabulary windows-only gem files declare with.
    /// Their widths are fixed by the Win32 API's own definitions on EVERY
    /// platform -- `DWORD` is 32 bits wherever the word is written -- so
    /// resolving them is not a platform guess. Ambiguous Win32 names
    /// (`LONG` is 32 there, but plain `long` here) are NOT included -- only
    /// the spellings that exist solely in the Win32 vocabulary.
    fn from_win32(name: &str) -> Option<CScalar> {
        Some(match name {
            "BYTE" | "BOOLEAN" | "UCHAR" => CScalar::U8,
            "WORD" | "USHORT" => CScalar::U16,
            "DWORD" | "dword" | "ULONG32" | "UINT32" => CScalar::U32,
            "DWORD64" | "ULONGLONG" | "DWORDLONG" | "ULONG64" => CScalar::U64,
            "LARGE_INTEGER" | "LONGLONG" | "LONG64" => CScalar::I64,
            "HANDLE" | "HWND" | "HINSTANCE" | "HMODULE" | "LPVOID" | "PVOID" | "FARPROC" => {
                CScalar::Pointer
            }
            "LPCSTR" | "LPSTR" | "LPCWSTR" | "LPWSTR" => CScalar::Pointer,
            "WPARAM" | "SIZE_T" => CScalar::U64,
            "LPARAM" | "SSIZE_T" => CScalar::I64,
            _ => return None,
        })
    }

    /// An `FFI::Type::X` / `FFI::Type::Builtin::X` constant's LEAF name.
    /// (`VARARGS` is a list terminator, not a type -- deliberately absent.)
    pub fn from_type_constant(leaf: &str) -> Option<CScalar> {
        Some(match leaf {
            "VOID" => CScalar::Void,
            "POINTER" => CScalar::Pointer,
            "STRING" => CScalar::Str,
            "BOOL" => CScalar::Bool,
            "CHAR" | "INT8" => CScalar::I8,
            "UCHAR" | "UINT8" => CScalar::U8,
            "SHORT" | "INT16" => CScalar::I16,
            "USHORT" | "UINT16" => CScalar::U16,
            "INT" | "INT32" => CScalar::I32,
            "UINT" | "UINT32" => CScalar::U32,
            "LONG_LONG" | "INT64" => CScalar::I64,
            "ULONG_LONG" | "UINT64" => CScalar::U64,
            "LONG" => CScalar::Long,
            "ULONG" => CScalar::ULong,
            "FLOAT" | "FLOAT32" => CScalar::F32,
            "DOUBLE" | "FLOAT64" => CScalar::F64,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_are_lp64() {
        assert_eq!(CScalar::from_keyword("long"), Some(CScalar::Long));
        assert_eq!(CScalar::from_keyword("size_t"), Some(CScalar::ULong));
        assert_eq!(CScalar::Long.size(), 8);
        assert_eq!(CScalar::I32.size(), 4);
        assert_eq!(CScalar::Pointer.size(), 8);
        assert_eq!(CScalar::Bool.size(), 1);
        assert_eq!(CScalar::Void.size(), 0);
    }

    #[test]
    fn typedef_spelling_prefixes_strip() {
        assert_eq!(CScalar::from_c_typedef("__int32_t"), Some(CScalar::I32));
        assert_eq!(CScalar::from_c_typedef("u_int32_t"), Some(CScalar::U32));
        assert_eq!(CScalar::from_c_typedef("mode_t"), None);
        assert_eq!(CScalar::from_c_typedef("DWORD"), Some(CScalar::U32));
    }

    #[test]
    fn type_constants_cover_both_int_spellings() {
        assert_eq!(CScalar::from_type_constant("LONG"), Some(CScalar::Long));
        assert_eq!(CScalar::from_type_constant("INT64"), Some(CScalar::I64));
        assert_eq!(CScalar::from_type_constant("VARARGS"), None);
    }
}
