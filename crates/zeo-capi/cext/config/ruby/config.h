#ifndef INCLUDE_RUBY_CONFIG_H
#define INCLUDE_RUBY_CONFIG_H 1
/*
 * zeo's `ruby/config.h`. Added by zeo; not part of upstream Ruby.
 *
 * MRI generates this file with autoconf, at the machine that builds the
 * interpreter. zeo has no such step: it is a compiler, it ships as a binary,
 * and the extension it builds is built on the user's machine long after.
 * So this file is written by hand and says only what is true of every target
 * zeo supports -- 64-bit macOS and Linux, aarch64 and x86_64 -- with an `#if`
 * ladder wherever the two disagree.
 *
 * That is a narrower promise than autoconf's, and a deliberate one. A `HAVE_`
 * left undefined makes `ruby/missing.h` declare a fallback prototype, which
 * is harmless. A `HAVE_` wrongly defined makes an extension call a function
 * that is not there, and the failure is a link error at the end of a long
 * build. When in doubt, leave it out.
 */

#if !defined(__APPLE__) && !defined(__linux__)
# error zeo builds C extensions on macOS and Linux only
#endif
#if defined(_WIN32) || defined(_WIN64)
# error zeo does not build C extensions on Windows
#endif

/* Headers. Every one of these is POSIX or C99. */
#define STDC_HEADERS 1
#define HAVE_STDIO_H 1
#define HAVE_STDLIB_H 1
#define HAVE_STDDEF_H 1
#define HAVE_STDARG_H 1
#define HAVE_STDBOOL_H 1
#define HAVE_STDINT_H 1
#define HAVE_STRING_H 1
#define HAVE_STRINGS_H 1
#define HAVE_INTTYPES_H 1
#define HAVE_LIMITS_H 1
#define HAVE_FLOAT_H 1
#define HAVE_MATH_H 1
#define HAVE_ERRNO_H 1
#define HAVE_FCNTL_H 1
#define HAVE_TIME_H 1
#define HAVE_UNISTD_H 1
#define HAVE_WCHAR_H 1
#define HAVE_DIRENT_H 1
#define HAVE_PTHREAD_H 1
#define HAVE_PWD_H 1
#define HAVE_GRP_H 1
#define HAVE_SYS_TYPES_H 1
#define HAVE_SYS_STAT_H 1
#define HAVE_SYS_TIME_H 1
#define HAVE_SYS_WAIT_H 1
#define HAVE_SYS_MMAN_H 1
#define HAVE_SYS_SELECT_H 1
#define HAVE_SYS_SOCKET_H 1
#define HAVE_SYS_IOCTL_H 1
#define HAVE_SYS_RESOURCE_H 1
#define HAVE_SYS_UIO_H 1
#define HAVE_NETINET_IN_H 1
#define HAVE_ARPA_INET_H 1
#define HAVE_NETDB_H 1
#define HAVE_POLL_H 1

/* Types. */
#define HAVE_INT8_T 1
#define HAVE_UINT8_T 1
#define HAVE_INT16_T 1
#define HAVE_UINT16_T 1
#define HAVE_INT32_T 1
#define HAVE_UINT32_T 1
#define HAVE_INT64_T 1
#define HAVE_UINT64_T 1
#define HAVE_INTPTR_T 1
#define HAVE_UINTPTR_T 1
#define HAVE_SSIZE_T 1
#define HAVE_LONG_LONG 1
#define HAVE_TRUE_LONG_LONG 1
#define HAVE_STRUCT_TIMEVAL 1
#define HAVE_STRUCT_TIMESPEC 1
#define HAVE_STRUCT_TIMEZONE 1
#define HAVE_STRUCT_TM_TM_GMTOFF 1
#define HAVE_TYPEOF 1
#define HAVE_STMT_AND_DECL_IN_EXPR 1
#define HAVE_VA_ARGS_MACRO 1

/* Widths. LP64 is the only model zeo targets, so these are not probed.
 * `long double` is the one real split: 80-bit extended on x86_64 (stored in
 * 16 bytes) and plain double on aarch64-darwin. */
#define SIZEOF_CHAR 1
#define SIZEOF_SHORT 2
#define SIZEOF_INT 4
#define SIZEOF_LONG 8
#define SIZEOF_LONG_LONG 8
#define SIZEOF_VOIDP 8
#define SIZEOF_FLOAT 4
#define SIZEOF_DOUBLE 8
#define SIZEOF_SIZE_T 8
#define SIZEOF_PTRDIFF_T 8
#define SIZEOF_TIME_T 8
#define SIZEOF_CLOCK_T 8
#define SIZEOF_OFF_T 8
#define SIZEOF_RLIM_T 8
#define SIZEOF____INT64 0
#define SIZEOF___INT64 0
#define SIZEOF_INT8_T 1
#define SIZEOF_UINT8_T 1
#define SIZEOF_INT16_T 2
#define SIZEOF_UINT16_T 2
#define SIZEOF_INT32_T 4
#define SIZEOF_UINT32_T 4
#define SIZEOF_INT64_T 8
#define SIZEOF_UINT64_T 8
#define SIZEOF_INTPTR_T 8
#define SIZEOF_UINTPTR_T 8
#define SIZEOF_PID_T 4
#define SIZEOF_UID_T 4
#define SIZEOF_GID_T 4
/* `dev_t` is the one width the two platforms disagree on: `int32_t` on
 * darwin, `unsigned long` on glibc. */
#ifdef __APPLE__
# define SIZEOF_DEV_T 4
#else
# define SIZEOF_DEV_T 8
#endif
#define SIZEOF_STRUCT_STAT_ST_SIZE SIZEOF_OFF_T
#define SIZEOF_STRUCT_STAT_ST_BLOCKS SIZEOF_OFF_T

#if defined(__x86_64__)
# define SIZEOF_LONG_DOUBLE 16
#else
# define SIZEOF_LONG_DOUBLE 8
#endif

#ifdef __APPLE__
# define SIZEOF_MODE_T 2
#else
# define SIZEOF_MODE_T 4
#endif

/* clang and gcc both offer __int128 on every target zeo builds for. */
#define SIZEOF___INT128 16
#define HAVE_INT128_T 1
#define int128_t __int128
#define SIZEOF_INT128_T SIZEOF___INT128
#define HAVE_UINT128_T 1
#define uint128_t unsigned __int128
#define SIZEOF_UINT128_T SIZEOF___INT128

#define HAVE_CLOCKID_T 1
#define SIZEOF_CLOCKID_T 4

/* The printf prefixes autoconf measures. `ruby/backward/2/inttypes.h` derives
 * the rest of the family from these three. */
#define PRI_LL_PREFIX "ll"
#define PRI_PTR_PREFIX "l"
#define PRI_SIZE_PREFIX "z"
#define PRI_PTRDIFF_PREFIX "t"

/* The `<type> <-> VALUE` conversions. MRI derives each from the width and
 * signedness autoconf measured; zeo states them, because it knows the four
 * targets it supports. A wrong choice here is silent -- a negative `pid_t`
 * would come back as a huge positive -- so each row names its C type.
 *
 * `off_t` is deliberately absent: `internal/arithmetic/off_t.h` picks it from
 * SIZEOF_OFF_T, and defining it here would only give one fact two owners. */
#define SIGNEDNESS_OF_PID_T -1               /* int */
#define PIDT2NUM(v) INT2NUM(v)
#define NUM2PIDT(v) NUM2INT(v)
#define PRI_PIDT_PREFIX PRI_INT_PREFIX

#define SIGNEDNESS_OF_UID_T +1               /* unsigned int */
#define UIDT2NUM(v) UINT2NUM(v)
#define NUM2UIDT(v) NUM2UINT(v)
#define PRI_UIDT_PREFIX PRI_INT_PREFIX

#define SIGNEDNESS_OF_GID_T +1               /* unsigned int */
#define GIDT2NUM(v) UINT2NUM(v)
#define NUM2GIDT(v) NUM2UINT(v)
#define PRI_GIDT_PREFIX PRI_INT_PREFIX
#define GETGROUPS_T gid_t

#define SIGNEDNESS_OF_TIME_T -1              /* long */
#define TIMET2NUM(v) LONG2NUM(v)
#define NUM2TIMET(v) NUM2LONG(v)
#define PRI_TIMET_PREFIX PRI_LONG_PREFIX

#define SIGNEDNESS_OF_RLIM_T +1              /* unsigned long long */
#define RLIM2NUM(v) ULL2NUM(v)
#define NUM2RLIM(v) NUM2ULL(v)
#define PRI_RLIM_PREFIX PRI_LL_PREFIX

#ifdef __APPLE__
# define SIGNEDNESS_OF_DEV_T -1              /* int32_t */
# define DEVT2NUM(v) INT2NUM(v)
# define NUM2DEVT(v) NUM2INT(v)
# define PRI_DEVT_PREFIX PRI_INT_PREFIX
# define SIGNEDNESS_OF_MODE_T +1             /* unsigned short */
# define MODET2NUM(v) USHORT2NUM(v)
# define NUM2MODET(v) NUM2USHORT(v)
# define PRI_MODET_PREFIX PRI_SHORT_PREFIX
# define SIGNEDNESS_OF_CLOCKID_T +1          /* unsigned int */
# define CLOCKID2NUM(v) UINT2NUM(v)
# define NUM2CLOCKID(v) NUM2UINT(v)
#else
# define SIGNEDNESS_OF_DEV_T +1              /* unsigned long */
# define DEVT2NUM(v) ULONG2NUM(v)
# define NUM2DEVT(v) NUM2ULONG(v)
# define PRI_DEVT_PREFIX PRI_LONG_PREFIX
# define SIGNEDNESS_OF_MODE_T +1             /* unsigned int */
# define MODET2NUM(v) UINT2NUM(v)
# define NUM2MODET(v) NUM2UINT(v)
# define PRI_MODET_PREFIX PRI_INT_PREFIX
/* glibc hands out NEGATIVE clock ids for the per-pid clocks. */
# define SIGNEDNESS_OF_CLOCKID_T -1          /* int */
# define CLOCKID2NUM(v) INT2NUM(v)
# define NUM2CLOCKID(v) NUM2INT(v)
#endif
#define PRI_CLOCKID_PREFIX PRI_INT_PREFIX

typedef int rb_pid_t;
#define rb_pid_t rb_pid_t
typedef unsigned int rb_uid_t;
#define rb_uid_t rb_uid_t
typedef unsigned int rb_gid_t;
#define rb_gid_t rb_gid_t
typedef long rb_off_t;
#define rb_off_t rb_off_t
#ifdef __APPLE__
typedef unsigned short rb_mode_t;
#else
typedef unsigned int rb_mode_t;
#endif
#define rb_mode_t rb_mode_t

/* libc and libm. Everything here is on both platforms; anything only one of
 * them has is left undefined on purpose -- `setproctitle` (BSD) and `eaccess`
 * (glibc) are the two that tempt. */
#define HAVE_ACOSH 1
#define HAVE_CBRT 1
#define HAVE_CHMOD 1
#define HAVE_CHOWN 1
#define HAVE_CRYPT 1
#define HAVE_DUP 1
#define HAVE_DUP2 1
#define HAVE_ERF 1
#define HAVE_EXECL 1
#define HAVE_EXECLE 1
#define HAVE_EXECV 1
#define HAVE_EXECVE 1
#define HAVE_EXPLICIT_BZERO 1
#define HAVE_FFS 1
#define HAVE_FINITE 1
#define HAVE_FLOCK 1
#define HAVE_FREXP 1
#define HAVE_GETEGID 1
#define HAVE_GETEUID 1
#define HAVE_GETGID 1
#define HAVE_GETLOGIN 1
#define HAVE_GETPPID 1
#define HAVE_GETUID 1
#define HAVE_GMTIME_R 1
#define HAVE_HYPOT 1
#define HAVE_KILL 1
#define HAVE_LGAMMA_R 1
#define HAVE_LOCALTIME_R 1
#define HAVE_MEMCMP 1
#define HAVE_MEMMOVE 1
#define HAVE_MODF 1
#define HAVE_NAN 1
#define HAVE_NEXTAFTER 1
#define HAVE_PCLOSE 1
#define HAVE_PIPE 1
#define HAVE_POPEN 1
#define HAVE_POSIX_MADVISE 1
#define HAVE_ROUND 1
#define HAVE_SHUTDOWN 1
#define HAVE_STRCHR 1
#define HAVE_STRERROR 1
#define HAVE_STRLCAT 1
#define HAVE_STRLCPY 1
#define HAVE_STRSTR 1
#define HAVE_SYSTEM 1
#define HAVE_TGAMMA 1
#define HAVE_TZSET 1
#define HAVE_UMASK 1
#define HAVE_WAITPID 1

/* Compiler features. zeo builds extensions with clang or gcc; both have
 * every one of these, and MSVC is not a target. */
#define HAVE_BUILTIN___BUILTIN_ALLOCA_WITH_ALIGN 1
#define HAVE_BUILTIN___BUILTIN_ASSUME_ALIGNED 1
#define HAVE_BUILTIN___BUILTIN_CHOOSE_EXPR 1
#define HAVE_BUILTIN___BUILTIN_CHOOSE_EXPR_CONSTANT_P 1
#define HAVE_BUILTIN___BUILTIN_CONSTANT_P 1
#define HAVE_BUILTIN___BUILTIN_EXPECT 1
#define HAVE_BUILTIN___BUILTIN_TYPES_COMPATIBLE_P 1
#define HAVE_BUILTIN___BUILTIN_UNREACHABLE 1
#define HAVE___BUILTIN_ADD_OVERFLOW 1
#define HAVE___BUILTIN_MUL_OVERFLOW 1
#define HAVE___BUILTIN_SUB_OVERFLOW 1
#define HAVE_GCC_ATOMIC_BUILTINS 1
#define HAVE_GCC_SYNC_BUILTINS 1
#define HAVE_ATTRIBUTE_FUNCTION_ALIAS 1

#define ENUM_OVER_INT 1
#define USE_UNALIGNED_MEMBER_ACCESS 1
#define RBIMPL_ATTR_PACKED_STRUCT_BEGIN()
#define RBIMPL_ATTR_PACKED_STRUCT_END() __attribute__((packed))
#define NO_SANITIZE(san, x) __attribute__ ((__no_sanitize__(san))) x
#define NO_SANITIZE_ADDRESS(x) __attribute__ ((__no_sanitize_address__)) x
#define NO_ADDRESS_SAFETY_ANALYSIS(x) __attribute__ ((__no_address_safety_analysis__)) x
#define ERRORFUNC(mesg, x) __attribute__ ((__error__ mesg)) x
#define WARNINGFUNC(mesg, x) __attribute__ ((__warning__ mesg)) x
#define WEAK(x) __attribute__ ((__weak__)) x

#define RUBY_ALIGNAS(x) _Alignas(x)
#define RUBY_ALIGNOF _Alignof
#define PACKED_STRUCT(x) x
#define PACKED_STRUCT_UNALIGNED(x) x
#define NORETURN(x) __attribute__ ((__noreturn__)) x
#define DEPRECATED(x) __attribute__ ((__deprecated__)) x
#define NOINLINE(x) __attribute__ ((__noinline__)) x
#define ALWAYS_INLINE(x) __attribute__ ((__always_inline__)) x
#define PUREFUNC(x) __attribute__ ((__pure__)) x
#define CONSTFUNC(x) __attribute__ ((__const__)) x
#define MAYBE_UNUSED(x) __attribute__ ((__unused__)) x
#define WARN_UNUSED_RESULT(x) __attribute__ ((__warn_unused_result__)) x
/* NOT UNREACHABLE / UNREACHABLE_RETURN: `ruby/backward/2/assume.h` owns
 * both, and MRI's own config.h leaves them to it. */
#define HAVE___BUILTIN_UNREACHABLE 1
#define FUNC_STDCALL(x) x
#define FUNC_CDECL(x) x
#define FUNC_FASTCALL(x) x
#define FUNC_UNOPTIMIZED(x) x
#define FUNC_MINIMIZED(x) x
#define RUBY_ALIAS_FUNCTION_TYPE(type, prot, name, args) \
    FUNC_MINIMIZED(type prot) {return name args;}
#define RUBY_ALIAS_FUNCTION(prot, name, args) \
    RUBY_ALIAS_FUNCTION_TYPE(VALUE, prot, name, args)

#define RUBY_SYMBOL_EXPORT_BEGIN _Pragma("GCC visibility push(default)")
#define RUBY_SYMBOL_EXPORT_END _Pragma("GCC visibility pop")
#define RUBY_EXTERN extern
#define RUBY_FUNC_EXPORTED __attribute__ ((__visibility__("default"))) extern

#define STRINGIZE(x) STRINGIZE0(x)
#define STRINGIZE0(x) #x

#endif /* INCLUDE_RUBY_CONFIG_H */
