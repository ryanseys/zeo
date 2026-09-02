//! `ruby/config.h`, rendered for one target from a table of facts.
//!
//! MRI generates this file with autoconf, at the machine that builds the
//! interpreter. zeo has no such step: it is a compiler, it ships as a
//! binary, and the extension it builds is built on the user's machine long
//! after. So the file says only what is true of every target zeo supports --
//! 64-bit macOS and Linux, aarch64 and x86_64 -- with a per-target value
//! wherever they disagree.
//!
//! That is a narrower promise than autoconf's, and a deliberate one. A
//! `HAVE_` left undefined makes `ruby/missing.h` declare a fallback
//! prototype, which is harmless. A `HAVE_` wrongly defined makes an extension
//! call a function that is not there, and the failure is a link error at the
//! end of a long build. When in doubt, leave it out.
//!
//! The macro bodies in the tables are the one accepted residue of C in this
//! repository: they exist so that no `.h` file has to.

use super::{Arch, Os, Target};

/// A definition's right-hand side, by target where the targets disagree.
enum Value {
    All(&'static str),
    ByOs {
        darwin: &'static str,
        linux: &'static str,
    },
    ByArch {
        aarch64: &'static str,
        x86_64: &'static str,
    },
}

impl Value {
    fn on(&self, t: Target) -> &'static str {
        match *self {
            Value::All(v) => v,
            Value::ByOs { darwin, linux } => match t.os {
                Os::Darwin => darwin,
                Os::Linux => linux,
            },
            Value::ByArch { aarch64, x86_64 } => match t.arch {
                Arch::Aarch64 => aarch64,
                Arch::X86_64 => x86_64,
            },
        }
    }
}

use Value::{All, ByArch, ByOs};

/// One `#define NAME VALUE` (the name may carry its parameter list), or a
/// `typedef` with the `#define name name` marker MRI's own config.h writes.
enum Line {
    Define(&'static str, Value),
    Typedef(&'static str, Value),
}

use Line::{Define, Typedef};

/// A commented group of lines.
struct Section {
    comment: &'static str,
    lines: &'static [Line],
}

macro_rules! have {
    ($($name:literal),* $(,)?) => { &[$(Define(concat!("HAVE_", $name), All("1"))),*] };
}

const SECTIONS: &[Section] = &[
    Section {
        comment: "Headers. Every one of these is POSIX or C99.",
        lines: have!(
            "STDIO_H",
            "STDLIB_H",
            "STDDEF_H",
            "STDARG_H",
            "STDBOOL_H",
            "STDINT_H",
            "STRING_H",
            "STRINGS_H",
            "INTTYPES_H",
            "LIMITS_H",
            "FLOAT_H",
            "MATH_H",
            "ERRNO_H",
            "FCNTL_H",
            "TIME_H",
            "UNISTD_H",
            "WCHAR_H",
            "DIRENT_H",
            "PTHREAD_H",
            "PWD_H",
            "GRP_H",
            "SYS_TYPES_H",
            "SYS_STAT_H",
            "SYS_TIME_H",
            "SYS_WAIT_H",
            "SYS_MMAN_H",
            "SYS_SELECT_H",
            "SYS_SOCKET_H",
            "SYS_IOCTL_H",
            "SYS_RESOURCE_H",
            "SYS_UIO_H",
            "NETINET_IN_H",
            "ARPA_INET_H",
            "NETDB_H",
            "POLL_H",
        ),
    },
    Section {
        comment: "Types.",
        lines: &[
            Define("STDC_HEADERS", All("1")),
            Define("HAVE_INT8_T", All("1")),
            Define("HAVE_UINT8_T", All("1")),
            Define("HAVE_INT16_T", All("1")),
            Define("HAVE_UINT16_T", All("1")),
            Define("HAVE_INT32_T", All("1")),
            Define("HAVE_UINT32_T", All("1")),
            Define("HAVE_INT64_T", All("1")),
            Define("HAVE_UINT64_T", All("1")),
            Define("HAVE_INTPTR_T", All("1")),
            Define("HAVE_UINTPTR_T", All("1")),
            Define("HAVE_SSIZE_T", All("1")),
            Define("HAVE_LONG_LONG", All("1")),
            Define("HAVE_TRUE_LONG_LONG", All("1")),
            Define("HAVE_STRUCT_TIMEVAL", All("1")),
            Define("HAVE_STRUCT_TIMESPEC", All("1")),
            Define("HAVE_STRUCT_TIMEZONE", All("1")),
            Define("HAVE_STRUCT_TM_TM_GMTOFF", All("1")),
            Define("HAVE_TYPEOF", All("1")),
            Define("HAVE_STMT_AND_DECL_IN_EXPR", All("1")),
            Define("HAVE_VA_ARGS_MACRO", All("1")),
        ],
    },
    Section {
        comment: "Widths. LP64 is the only model zeo targets, so these are not probed.\n\
                  \x20* `long double` is the one real split: 80-bit extended on x86_64 (stored\n\
                  \x20* in 16 bytes) and plain double on aarch64. `dev_t` and `mode_t` are the\n\
                  \x20* two the platforms disagree on.",
        lines: &[
            Define("SIZEOF_CHAR", All("1")),
            Define("SIZEOF_SHORT", All("2")),
            Define("SIZEOF_INT", All("4")),
            Define("SIZEOF_LONG", All("8")),
            Define("SIZEOF_LONG_LONG", All("8")),
            Define("SIZEOF_VOIDP", All("8")),
            Define("SIZEOF_FLOAT", All("4")),
            Define("SIZEOF_DOUBLE", All("8")),
            Define("SIZEOF_SIZE_T", All("8")),
            Define("SIZEOF_PTRDIFF_T", All("8")),
            Define("SIZEOF_TIME_T", All("8")),
            Define("SIZEOF_CLOCK_T", All("8")),
            Define("SIZEOF_OFF_T", All("8")),
            Define("SIZEOF_RLIM_T", All("8")),
            Define("SIZEOF____INT64", All("0")),
            Define("SIZEOF___INT64", All("0")),
            Define("SIZEOF_INT8_T", All("1")),
            Define("SIZEOF_UINT8_T", All("1")),
            Define("SIZEOF_INT16_T", All("2")),
            Define("SIZEOF_UINT16_T", All("2")),
            Define("SIZEOF_INT32_T", All("4")),
            Define("SIZEOF_UINT32_T", All("4")),
            Define("SIZEOF_INT64_T", All("8")),
            Define("SIZEOF_UINT64_T", All("8")),
            Define("SIZEOF_INTPTR_T", All("8")),
            Define("SIZEOF_UINTPTR_T", All("8")),
            Define("SIZEOF_PID_T", All("4")),
            Define("SIZEOF_UID_T", All("4")),
            Define("SIZEOF_GID_T", All("4")),
            Define(
                "SIZEOF_DEV_T",
                ByOs {
                    darwin: "4",
                    linux: "8",
                },
            ),
            Define("SIZEOF_STRUCT_STAT_ST_SIZE", All("SIZEOF_OFF_T")),
            Define("SIZEOF_STRUCT_STAT_ST_BLOCKS", All("SIZEOF_OFF_T")),
            Define(
                "SIZEOF_LONG_DOUBLE",
                ByArch {
                    aarch64: "8",
                    x86_64: "16",
                },
            ),
            Define(
                "SIZEOF_MODE_T",
                ByOs {
                    darwin: "2",
                    linux: "4",
                },
            ),
            Define("SIZEOF___INT128", All("16")),
            Define("HAVE_INT128_T", All("1")),
            Define("int128_t", All("__int128")),
            Define("SIZEOF_INT128_T", All("SIZEOF___INT128")),
            Define("HAVE_UINT128_T", All("1")),
            Define("uint128_t", All("unsigned __int128")),
            Define("SIZEOF_UINT128_T", All("SIZEOF___INT128")),
            Define("HAVE_CLOCKID_T", All("1")),
            Define("SIZEOF_CLOCKID_T", All("4")),
        ],
    },
    Section {
        comment: "The printf prefixes autoconf measures. `ruby/backward/2/inttypes.h`\n\
                  \x20* derives the rest of the family from these.",
        lines: &[
            Define("PRI_LL_PREFIX", All("\"ll\"")),
            Define("PRI_PTR_PREFIX", All("\"l\"")),
            Define("PRI_SIZE_PREFIX", All("\"z\"")),
            Define("PRI_PTRDIFF_PREFIX", All("\"t\"")),
        ],
    },
    Section {
        comment: "The `<type> <-> VALUE` conversions. MRI derives each from the width and\n\
                  \x20* signedness autoconf measured; zeo states them, because it knows the\n\
                  \x20* targets it supports. A wrong choice here is silent -- a negative `pid_t`\n\
                  \x20* would come back as a huge positive -- so each row names its C type.\n\
                  \x20*\n\
                  \x20* `off_t` is deliberately absent: `internal/arithmetic/off_t.h` picks it\n\
                  \x20* from SIZEOF_OFF_T, and defining it here would give one fact two owners.\n\
                  \x20* glibc hands out NEGATIVE clock ids for the per-pid clocks.",
        lines: &[
            Define("SIGNEDNESS_OF_PID_T", All("-1               /* int */")),
            Define("PIDT2NUM(v)", All("INT2NUM(v)")),
            Define("NUM2PIDT(v)", All("NUM2INT(v)")),
            Define("PRI_PIDT_PREFIX", All("PRI_INT_PREFIX")),
            Define(
                "SIGNEDNESS_OF_UID_T",
                All("+1               /* unsigned int */"),
            ),
            Define("UIDT2NUM(v)", All("UINT2NUM(v)")),
            Define("NUM2UIDT(v)", All("NUM2UINT(v)")),
            Define("PRI_UIDT_PREFIX", All("PRI_INT_PREFIX")),
            Define(
                "SIGNEDNESS_OF_GID_T",
                All("+1               /* unsigned int */"),
            ),
            Define("GIDT2NUM(v)", All("UINT2NUM(v)")),
            Define("NUM2GIDT(v)", All("NUM2UINT(v)")),
            Define("PRI_GIDT_PREFIX", All("PRI_INT_PREFIX")),
            Define("GETGROUPS_T", All("gid_t")),
            Define("SIGNEDNESS_OF_TIME_T", All("-1              /* long */")),
            Define("TIMET2NUM(v)", All("LONG2NUM(v)")),
            Define("NUM2TIMET(v)", All("NUM2LONG(v)")),
            Define("PRI_TIMET_PREFIX", All("PRI_LONG_PREFIX")),
            Define(
                "SIGNEDNESS_OF_RLIM_T",
                All("+1              /* unsigned long long */"),
            ),
            Define("RLIM2NUM(v)", All("ULL2NUM(v)")),
            Define("NUM2RLIM(v)", All("NUM2ULL(v)")),
            Define("PRI_RLIM_PREFIX", All("PRI_LL_PREFIX")),
            Define(
                "SIGNEDNESS_OF_DEV_T",
                ByOs {
                    darwin: "-1              /* int32_t */",
                    linux: "+1              /* unsigned long */",
                },
            ),
            Define(
                "DEVT2NUM(v)",
                ByOs {
                    darwin: "INT2NUM(v)",
                    linux: "ULONG2NUM(v)",
                },
            ),
            Define(
                "NUM2DEVT(v)",
                ByOs {
                    darwin: "NUM2INT(v)",
                    linux: "NUM2ULONG(v)",
                },
            ),
            Define(
                "PRI_DEVT_PREFIX",
                ByOs {
                    darwin: "PRI_INT_PREFIX",
                    linux: "PRI_LONG_PREFIX",
                },
            ),
            Define(
                "SIGNEDNESS_OF_MODE_T",
                ByOs {
                    darwin: "+1             /* unsigned short */",
                    linux: "+1             /* unsigned int */",
                },
            ),
            Define(
                "MODET2NUM(v)",
                ByOs {
                    darwin: "USHORT2NUM(v)",
                    linux: "UINT2NUM(v)",
                },
            ),
            Define(
                "NUM2MODET(v)",
                ByOs {
                    darwin: "NUM2USHORT(v)",
                    linux: "NUM2UINT(v)",
                },
            ),
            Define(
                "PRI_MODET_PREFIX",
                ByOs {
                    darwin: "PRI_SHORT_PREFIX",
                    linux: "PRI_INT_PREFIX",
                },
            ),
            Define(
                "SIGNEDNESS_OF_CLOCKID_T",
                ByOs {
                    darwin: "+1          /* unsigned int */",
                    linux: "-1          /* int */",
                },
            ),
            Define(
                "CLOCKID2NUM(v)",
                ByOs {
                    darwin: "UINT2NUM(v)",
                    linux: "INT2NUM(v)",
                },
            ),
            Define(
                "NUM2CLOCKID(v)",
                ByOs {
                    darwin: "NUM2UINT(v)",
                    linux: "NUM2INT(v)",
                },
            ),
            Define("PRI_CLOCKID_PREFIX", All("PRI_INT_PREFIX")),
            Typedef("rb_pid_t", All("int")),
            Typedef("rb_uid_t", All("unsigned int")),
            Typedef("rb_gid_t", All("unsigned int")),
            Typedef("rb_off_t", All("long")),
            Typedef(
                "rb_mode_t",
                ByOs {
                    darwin: "unsigned short",
                    linux: "unsigned int",
                },
            ),
        ],
    },
    Section {
        comment: "libc and libm. Everything here is on both platforms; anything only one of\n\
                  \x20* them has is left undefined on purpose -- `setproctitle` (BSD) and\n\
                  \x20* `eaccess` (glibc) are the two that tempt.",
        lines: have!(
            "ACOSH",
            "CBRT",
            "CHMOD",
            "CHOWN",
            "CRYPT",
            "DUP",
            "DUP2",
            "ERF",
            "EXECL",
            "EXECLE",
            "EXECV",
            "EXECVE",
            "EXPLICIT_BZERO",
            "FFS",
            "FINITE",
            "FLOCK",
            "FREXP",
            "GETEGID",
            "GETEUID",
            "GETGID",
            "GETLOGIN",
            "GETPPID",
            "GETUID",
            "GMTIME_R",
            "HYPOT",
            "KILL",
            "LGAMMA_R",
            "LOCALTIME_R",
            "MEMCMP",
            "MEMMOVE",
            "MODF",
            "NAN",
            "NEXTAFTER",
            "PCLOSE",
            "PIPE",
            "POPEN",
            "POSIX_MADVISE",
            "ROUND",
            "SHUTDOWN",
            "STRCHR",
            "STRERROR",
            "STRLCAT",
            "STRLCPY",
            "STRSTR",
            "SYSTEM",
            "TGAMMA",
            "TZSET",
            "UMASK",
            "WAITPID",
        ),
    },
    Section {
        comment: "Compiler features. zeo builds extensions with clang or gcc; both have\n\
                  \x20* every one of these, and MSVC is not a target. NOT UNREACHABLE /\n\
                  \x20* UNREACHABLE_RETURN: `ruby/backward/2/assume.h` owns both, and MRI's own\n\
                  \x20* config.h leaves them to it.",
        lines: &[
            Define("HAVE_BUILTIN___BUILTIN_ALLOCA_WITH_ALIGN", All("1")),
            Define("HAVE_BUILTIN___BUILTIN_ASSUME_ALIGNED", All("1")),
            Define("HAVE_BUILTIN___BUILTIN_CHOOSE_EXPR", All("1")),
            Define("HAVE_BUILTIN___BUILTIN_CHOOSE_EXPR_CONSTANT_P", All("1")),
            Define("HAVE_BUILTIN___BUILTIN_CONSTANT_P", All("1")),
            Define("HAVE_BUILTIN___BUILTIN_EXPECT", All("1")),
            Define("HAVE_BUILTIN___BUILTIN_TYPES_COMPATIBLE_P", All("1")),
            Define("HAVE_BUILTIN___BUILTIN_UNREACHABLE", All("1")),
            Define("HAVE___BUILTIN_ADD_OVERFLOW", All("1")),
            Define("HAVE___BUILTIN_MUL_OVERFLOW", All("1")),
            Define("HAVE___BUILTIN_SUB_OVERFLOW", All("1")),
            Define("HAVE___BUILTIN_UNREACHABLE", All("1")),
            Define("HAVE_GCC_ATOMIC_BUILTINS", All("1")),
            Define("HAVE_GCC_SYNC_BUILTINS", All("1")),
            Define("HAVE_ATTRIBUTE_FUNCTION_ALIAS", All("1")),
            Define("ENUM_OVER_INT", All("1")),
            Define("USE_UNALIGNED_MEMBER_ACCESS", All("1")),
            Define("RBIMPL_ATTR_PACKED_STRUCT_BEGIN()", All("")),
            Define(
                "RBIMPL_ATTR_PACKED_STRUCT_END()",
                All("__attribute__((packed))"),
            ),
            Define(
                "NO_SANITIZE(san, x)",
                All("__attribute__ ((__no_sanitize__(san))) x"),
            ),
            Define(
                "NO_SANITIZE_ADDRESS(x)",
                All("__attribute__ ((__no_sanitize_address__)) x"),
            ),
            Define(
                "NO_ADDRESS_SAFETY_ANALYSIS(x)",
                All("__attribute__ ((__no_address_safety_analysis__)) x"),
            ),
            Define(
                "ERRORFUNC(mesg, x)",
                All("__attribute__ ((__error__ mesg)) x"),
            ),
            Define(
                "WARNINGFUNC(mesg, x)",
                All("__attribute__ ((__warning__ mesg)) x"),
            ),
            Define("WEAK(x)", All("__attribute__ ((__weak__)) x")),
            Define("RUBY_ALIGNAS(x)", All("_Alignas(x)")),
            Define("RUBY_ALIGNOF", All("_Alignof")),
            Define("PACKED_STRUCT(x)", All("x")),
            Define("PACKED_STRUCT_UNALIGNED(x)", All("x")),
            Define("NORETURN(x)", All("__attribute__ ((__noreturn__)) x")),
            Define("DEPRECATED(x)", All("__attribute__ ((__deprecated__)) x")),
            Define("NOINLINE(x)", All("__attribute__ ((__noinline__)) x")),
            Define(
                "ALWAYS_INLINE(x)",
                All("__attribute__ ((__always_inline__)) x"),
            ),
            Define("PUREFUNC(x)", All("__attribute__ ((__pure__)) x")),
            Define("CONSTFUNC(x)", All("__attribute__ ((__const__)) x")),
            Define("MAYBE_UNUSED(x)", All("__attribute__ ((__unused__)) x")),
            Define(
                "WARN_UNUSED_RESULT(x)",
                All("__attribute__ ((__warn_unused_result__)) x"),
            ),
            Define("FUNC_STDCALL(x)", All("x")),
            Define("FUNC_CDECL(x)", All("x")),
            Define("FUNC_FASTCALL(x)", All("x")),
            Define("FUNC_UNOPTIMIZED(x)", All("x")),
            Define("FUNC_MINIMIZED(x)", All("x")),
            Define(
                "RUBY_ALIAS_FUNCTION_TYPE(type, prot, name, args)",
                All("\\\n    FUNC_MINIMIZED(type prot) {return name args;}"),
            ),
            Define(
                "RUBY_ALIAS_FUNCTION(prot, name, args)",
                All("\\\n    RUBY_ALIAS_FUNCTION_TYPE(VALUE, prot, name, args)"),
            ),
            Define(
                "RUBY_SYMBOL_EXPORT_BEGIN",
                All("_Pragma(\"GCC visibility push(default)\")"),
            ),
            Define(
                "RUBY_SYMBOL_EXPORT_END",
                All("_Pragma(\"GCC visibility pop\")"),
            ),
            Define("RUBY_EXTERN", All("extern")),
            Define(
                "RUBY_FUNC_EXPORTED",
                All("__attribute__ ((__visibility__(\"default\"))) extern"),
            ),
            Define("STRINGIZE(x)", All("STRINGIZE0(x)")),
            Define("STRINGIZE0(x)", All("#x")),
        ],
    },
];

/// The header's text for `target`.
pub fn render(target: Target) -> String {
    let (os, arch) = (
        match target.os {
            Os::Darwin => "macOS",
            Os::Linux => "Linux",
        },
        match target.arch {
            Arch::Aarch64 => "aarch64",
            Arch::X86_64 => "x86_64",
        },
    );
    let mut h = format!(
        "#ifndef INCLUDE_RUBY_CONFIG_H\n\
         #define INCLUDE_RUBY_CONFIG_H 1\n\
         /*\n\
         \x20* zeo's `ruby/config.h`, rendered for {os} {arch}. Added by zeo; not part\n\
         \x20* of upstream Ruby.\n\
         \x20*\n\
         \x20* MRI generates this file with autoconf, at the machine that builds the\n\
         \x20* interpreter. zeo has no such step: it renders this one for the host from\n\
         \x20* a table of facts (`zeo-capi/src/headers/config_h.rs`), and says only\n\
         \x20* what is true of that host. A `HAVE_` left undefined makes `ruby/missing.h`\n\
         \x20* declare a fallback prototype, which is harmless; a `HAVE_` wrongly\n\
         \x20* defined is a link error at the end of a long build.\n\
         \x20*/\n"
    );
    h.push_str(match target.os {
        Os::Darwin => {
            "#if !defined(__APPLE__)\n# error this config.h was rendered for macOS\n#endif\n"
        }
        Os::Linux => {
            "#if !defined(__linux__)\n# error this config.h was rendered for Linux\n#endif\n"
        }
    });
    for section in SECTIONS {
        h.push_str(&format!("/* {} */\n", section.comment));
        for line in section.lines {
            match line {
                Define(name, value) => {
                    let v = value.on(target);
                    if v.is_empty() {
                        h.push_str(&format!("#define {name}\n"));
                    } else {
                        h.push_str(&format!("#define {name} {v}\n"));
                    }
                }
                Typedef(name, ty) => {
                    h.push_str(&format!(
                        "typedef {} {name};\n#define {name} {name}\n",
                        ty.on(target)
                    ));
                }
            }
        }
    }
    h.push_str("#endif /* INCLUDE_RUBY_CONFIG_H */\n");
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Target; 4] = [
        Target {
            os: Os::Darwin,
            arch: Arch::Aarch64,
        },
        Target {
            os: Os::Darwin,
            arch: Arch::X86_64,
        },
        Target {
            os: Os::Linux,
            arch: Arch::Aarch64,
        },
        Target {
            os: Os::Linux,
            arch: Arch::X86_64,
        },
    ];

    /// Every target renders, no name is defined twice, and the rows that
    /// differ by target come out differently.
    #[test]
    fn every_target_renders_each_name_once() {
        for t in ALL {
            let h = render(t);
            let mut seen = std::collections::BTreeSet::new();
            for line in h.lines().filter(|l| l.starts_with("#define ")) {
                let name = line["#define ".len()..].split([' ', '(']).next().unwrap();
                assert!(
                    seen.insert(name.to_string()),
                    "{name} defined twice for {t:?}"
                );
            }
            assert!(h.contains("#define SIZEOF_VOIDP 8\n"));
            assert!(h.contains("typedef int rb_pid_t;\n#define rb_pid_t rb_pid_t\n"));
        }
        assert!(render(ALL[0]).contains("#define SIZEOF_MODE_T 2\n"));
        assert!(render(ALL[2]).contains("#define SIZEOF_MODE_T 4\n"));
        assert!(render(ALL[0]).contains("#define SIZEOF_LONG_DOUBLE 8\n"));
        assert!(render(ALL[1]).contains("#define SIZEOF_LONG_DOUBLE 16\n"));
        assert!(render(ALL[3]).contains("#define NUM2CLOCKID(v) NUM2INT(v)\n"));
    }
}
