//! The `Errno` classes -- the one table the compiler, the runtime and the
//! exception hierarchy all read.
//!
//! CRuby names one class per `errno` the platform defines, plus `NOERROR` for
//! zero, and binds every remaining name in its fixed list to an existing class:
//! a second spelling of the same value (`EWOULDBLOCK` is `EAGAIN`) or, for a
//! name the platform does not define at all, `NOERROR`. zeo reproduces both
//! halves, so `Errno.constants` answers the same 158 names and
//! `rescue Errno::EWOULDBLOCK` still catches an `EAGAIN`.
//!
//! The values are written out rather than read from `libc` because this crate
//! is the zero-dependency leaf both sides share. They are ABI-frozen, so a
//! literal cannot rot -- but they ARE per-platform, hence the two tables.

/// One `Errno` class: the name CRuby gives it and the platform's `errno`
/// value, which the class reports as its own `Errno` constant and each
/// instance reports as `#errno`.
#[derive(Clone, Copy)]
pub struct ErrnoClass {
    /// Fully-qualified Ruby name (`"Errno::ENOENT"`).
    pub name: &'static str,
    /// The platform `errno` value. Zero only for `Errno::NOERROR`.
    pub errno: i32,
}

/// Every distinct `Errno` class, sorted by name -- so the block takes a
/// contiguous, stable run of [`crate::EXCEPTION_CLASSES`] ids.
///
/// This is macOS's set, dumped from ruby 4.0.6 and identical to it.
#[cfg(target_vendor = "apple")]
pub const ERRNO_CLASSES: &[ErrnoClass] = &[
    ErrnoClass {
        name: "Errno::E2BIG",
        errno: 7,
    },
    ErrnoClass {
        name: "Errno::EACCES",
        errno: 13,
    },
    ErrnoClass {
        name: "Errno::EADDRINUSE",
        errno: 48,
    },
    ErrnoClass {
        name: "Errno::EADDRNOTAVAIL",
        errno: 49,
    },
    ErrnoClass {
        name: "Errno::EAFNOSUPPORT",
        errno: 47,
    },
    ErrnoClass {
        name: "Errno::EAGAIN",
        errno: 35,
    },
    ErrnoClass {
        name: "Errno::EALREADY",
        errno: 37,
    },
    ErrnoClass {
        name: "Errno::EAUTH",
        errno: 80,
    },
    ErrnoClass {
        name: "Errno::EBADARCH",
        errno: 86,
    },
    ErrnoClass {
        name: "Errno::EBADEXEC",
        errno: 85,
    },
    ErrnoClass {
        name: "Errno::EBADF",
        errno: 9,
    },
    ErrnoClass {
        name: "Errno::EBADMACHO",
        errno: 88,
    },
    ErrnoClass {
        name: "Errno::EBADMSG",
        errno: 94,
    },
    ErrnoClass {
        name: "Errno::EBADRPC",
        errno: 72,
    },
    ErrnoClass {
        name: "Errno::EBUSY",
        errno: 16,
    },
    ErrnoClass {
        name: "Errno::ECANCELED",
        errno: 89,
    },
    ErrnoClass {
        name: "Errno::ECHILD",
        errno: 10,
    },
    ErrnoClass {
        name: "Errno::ECONNABORTED",
        errno: 53,
    },
    ErrnoClass {
        name: "Errno::ECONNREFUSED",
        errno: 61,
    },
    ErrnoClass {
        name: "Errno::ECONNRESET",
        errno: 54,
    },
    ErrnoClass {
        name: "Errno::EDEADLK",
        errno: 11,
    },
    ErrnoClass {
        name: "Errno::EDESTADDRREQ",
        errno: 39,
    },
    ErrnoClass {
        name: "Errno::EDEVERR",
        errno: 83,
    },
    ErrnoClass {
        name: "Errno::EDOM",
        errno: 33,
    },
    ErrnoClass {
        name: "Errno::EDQUOT",
        errno: 69,
    },
    ErrnoClass {
        name: "Errno::EEXIST",
        errno: 17,
    },
    ErrnoClass {
        name: "Errno::EFAULT",
        errno: 14,
    },
    ErrnoClass {
        name: "Errno::EFBIG",
        errno: 27,
    },
    ErrnoClass {
        name: "Errno::EFTYPE",
        errno: 79,
    },
    ErrnoClass {
        name: "Errno::EHOSTDOWN",
        errno: 64,
    },
    ErrnoClass {
        name: "Errno::EHOSTUNREACH",
        errno: 65,
    },
    ErrnoClass {
        name: "Errno::EIDRM",
        errno: 90,
    },
    ErrnoClass {
        name: "Errno::EILSEQ",
        errno: 92,
    },
    ErrnoClass {
        name: "Errno::EINPROGRESS",
        errno: 36,
    },
    ErrnoClass {
        name: "Errno::EINTR",
        errno: 4,
    },
    ErrnoClass {
        name: "Errno::EINVAL",
        errno: 22,
    },
    ErrnoClass {
        name: "Errno::EIO",
        errno: 5,
    },
    ErrnoClass {
        name: "Errno::EISCONN",
        errno: 56,
    },
    ErrnoClass {
        name: "Errno::EISDIR",
        errno: 21,
    },
    ErrnoClass {
        name: "Errno::ELOOP",
        errno: 62,
    },
    ErrnoClass {
        name: "Errno::EMFILE",
        errno: 24,
    },
    ErrnoClass {
        name: "Errno::EMLINK",
        errno: 31,
    },
    ErrnoClass {
        name: "Errno::EMSGSIZE",
        errno: 40,
    },
    ErrnoClass {
        name: "Errno::EMULTIHOP",
        errno: 95,
    },
    ErrnoClass {
        name: "Errno::ENAMETOOLONG",
        errno: 63,
    },
    ErrnoClass {
        name: "Errno::ENEEDAUTH",
        errno: 81,
    },
    ErrnoClass {
        name: "Errno::ENETDOWN",
        errno: 50,
    },
    ErrnoClass {
        name: "Errno::ENETRESET",
        errno: 52,
    },
    ErrnoClass {
        name: "Errno::ENETUNREACH",
        errno: 51,
    },
    ErrnoClass {
        name: "Errno::ENFILE",
        errno: 23,
    },
    ErrnoClass {
        name: "Errno::ENOATTR",
        errno: 93,
    },
    ErrnoClass {
        name: "Errno::ENOBUFS",
        errno: 55,
    },
    ErrnoClass {
        name: "Errno::ENODATA",
        errno: 96,
    },
    ErrnoClass {
        name: "Errno::ENODEV",
        errno: 19,
    },
    ErrnoClass {
        name: "Errno::ENOENT",
        errno: 2,
    },
    ErrnoClass {
        name: "Errno::ENOEXEC",
        errno: 8,
    },
    ErrnoClass {
        name: "Errno::ENOLCK",
        errno: 77,
    },
    ErrnoClass {
        name: "Errno::ENOLINK",
        errno: 97,
    },
    ErrnoClass {
        name: "Errno::ENOMEM",
        errno: 12,
    },
    ErrnoClass {
        name: "Errno::ENOMSG",
        errno: 91,
    },
    ErrnoClass {
        name: "Errno::ENOPOLICY",
        errno: 103,
    },
    ErrnoClass {
        name: "Errno::ENOPROTOOPT",
        errno: 42,
    },
    ErrnoClass {
        name: "Errno::ENOSPC",
        errno: 28,
    },
    ErrnoClass {
        name: "Errno::ENOSR",
        errno: 98,
    },
    ErrnoClass {
        name: "Errno::ENOSTR",
        errno: 99,
    },
    ErrnoClass {
        name: "Errno::ENOSYS",
        errno: 78,
    },
    ErrnoClass {
        name: "Errno::ENOTBLK",
        errno: 15,
    },
    ErrnoClass {
        name: "Errno::ENOTCONN",
        errno: 57,
    },
    ErrnoClass {
        name: "Errno::ENOTDIR",
        errno: 20,
    },
    ErrnoClass {
        name: "Errno::ENOTEMPTY",
        errno: 66,
    },
    ErrnoClass {
        name: "Errno::ENOTRECOVERABLE",
        errno: 104,
    },
    ErrnoClass {
        name: "Errno::ENOTSOCK",
        errno: 38,
    },
    ErrnoClass {
        name: "Errno::ENOTSUP",
        errno: 45,
    },
    ErrnoClass {
        name: "Errno::ENOTTY",
        errno: 25,
    },
    ErrnoClass {
        name: "Errno::ENXIO",
        errno: 6,
    },
    ErrnoClass {
        name: "Errno::EOPNOTSUPP",
        errno: 102,
    },
    ErrnoClass {
        name: "Errno::EOVERFLOW",
        errno: 84,
    },
    ErrnoClass {
        name: "Errno::EOWNERDEAD",
        errno: 105,
    },
    ErrnoClass {
        name: "Errno::EPERM",
        errno: 1,
    },
    ErrnoClass {
        name: "Errno::EPFNOSUPPORT",
        errno: 46,
    },
    ErrnoClass {
        name: "Errno::EPIPE",
        errno: 32,
    },
    ErrnoClass {
        name: "Errno::EPROCLIM",
        errno: 67,
    },
    ErrnoClass {
        name: "Errno::EPROCUNAVAIL",
        errno: 76,
    },
    ErrnoClass {
        name: "Errno::EPROGMISMATCH",
        errno: 75,
    },
    ErrnoClass {
        name: "Errno::EPROGUNAVAIL",
        errno: 74,
    },
    ErrnoClass {
        name: "Errno::EPROTO",
        errno: 100,
    },
    ErrnoClass {
        name: "Errno::EPROTONOSUPPORT",
        errno: 43,
    },
    ErrnoClass {
        name: "Errno::EPROTOTYPE",
        errno: 41,
    },
    ErrnoClass {
        name: "Errno::EPWROFF",
        errno: 82,
    },
    ErrnoClass {
        name: "Errno::EQFULL",
        errno: 106,
    },
    ErrnoClass {
        name: "Errno::ERANGE",
        errno: 34,
    },
    ErrnoClass {
        name: "Errno::EREMOTE",
        errno: 71,
    },
    ErrnoClass {
        name: "Errno::EROFS",
        errno: 30,
    },
    ErrnoClass {
        name: "Errno::ERPCMISMATCH",
        errno: 73,
    },
    ErrnoClass {
        name: "Errno::ESHLIBVERS",
        errno: 87,
    },
    ErrnoClass {
        name: "Errno::ESHUTDOWN",
        errno: 58,
    },
    ErrnoClass {
        name: "Errno::ESOCKTNOSUPPORT",
        errno: 44,
    },
    ErrnoClass {
        name: "Errno::ESPIPE",
        errno: 29,
    },
    ErrnoClass {
        name: "Errno::ESRCH",
        errno: 3,
    },
    ErrnoClass {
        name: "Errno::ESTALE",
        errno: 70,
    },
    ErrnoClass {
        name: "Errno::ETIME",
        errno: 101,
    },
    ErrnoClass {
        name: "Errno::ETIMEDOUT",
        errno: 60,
    },
    ErrnoClass {
        name: "Errno::ETOOMANYREFS",
        errno: 59,
    },
    ErrnoClass {
        name: "Errno::ETXTBSY",
        errno: 26,
    },
    ErrnoClass {
        name: "Errno::EUSERS",
        errno: 68,
    },
    ErrnoClass {
        name: "Errno::EXDEV",
        errno: 18,
    },
    ErrnoClass {
        name: "Errno::NOERROR",
        errno: 0,
    },
];

/// The second names, each pointing at the [`ERRNO_CLASSES`] entry it
/// duplicates. `Errno::EWOULDBLOCK` IS `Errno::EAGAIN` (one value, two
/// spellings); the rest name an errno macOS does not define, which CRuby binds
/// to `Errno::NOERROR`.
#[cfg(target_vendor = "apple")]
pub const ERRNO_ALIASES: &[(&str, &str)] = &[
    ("Errno::EADV", "Errno::NOERROR"),
    ("Errno::EBADE", "Errno::NOERROR"),
    ("Errno::EBADFD", "Errno::NOERROR"),
    ("Errno::EBADR", "Errno::NOERROR"),
    ("Errno::EBADRQC", "Errno::NOERROR"),
    ("Errno::EBADSLT", "Errno::NOERROR"),
    ("Errno::EBFONT", "Errno::NOERROR"),
    ("Errno::ECAPMODE", "Errno::NOERROR"),
    ("Errno::ECHRNG", "Errno::NOERROR"),
    ("Errno::ECOMM", "Errno::NOERROR"),
    ("Errno::EDEADLOCK", "Errno::NOERROR"),
    ("Errno::EDOOFUS", "Errno::NOERROR"),
    ("Errno::EDOTDOT", "Errno::NOERROR"),
    ("Errno::EHWPOISON", "Errno::NOERROR"),
    ("Errno::EIPSEC", "Errno::NOERROR"),
    ("Errno::EISNAM", "Errno::NOERROR"),
    ("Errno::EKEYEXPIRED", "Errno::NOERROR"),
    ("Errno::EKEYREJECTED", "Errno::NOERROR"),
    ("Errno::EKEYREVOKED", "Errno::NOERROR"),
    ("Errno::EL2HLT", "Errno::NOERROR"),
    ("Errno::EL2NSYNC", "Errno::NOERROR"),
    ("Errno::EL3HLT", "Errno::NOERROR"),
    ("Errno::EL3RST", "Errno::NOERROR"),
    ("Errno::ELAST", "Errno::EQFULL"),
    ("Errno::ELIBACC", "Errno::NOERROR"),
    ("Errno::ELIBBAD", "Errno::NOERROR"),
    ("Errno::ELIBEXEC", "Errno::NOERROR"),
    ("Errno::ELIBMAX", "Errno::NOERROR"),
    ("Errno::ELIBSCN", "Errno::NOERROR"),
    ("Errno::ELNRNG", "Errno::NOERROR"),
    ("Errno::EMEDIUMTYPE", "Errno::NOERROR"),
    ("Errno::ENAVAIL", "Errno::NOERROR"),
    ("Errno::ENOANO", "Errno::NOERROR"),
    ("Errno::ENOCSI", "Errno::NOERROR"),
    ("Errno::ENOKEY", "Errno::NOERROR"),
    ("Errno::ENOMEDIUM", "Errno::NOERROR"),
    ("Errno::ENONET", "Errno::NOERROR"),
    ("Errno::ENOPKG", "Errno::NOERROR"),
    ("Errno::ENOTCAPABLE", "Errno::NOERROR"),
    ("Errno::ENOTNAM", "Errno::NOERROR"),
    ("Errno::ENOTUNIQ", "Errno::NOERROR"),
    ("Errno::EREMCHG", "Errno::NOERROR"),
    ("Errno::EREMOTEIO", "Errno::NOERROR"),
    ("Errno::ERESTART", "Errno::NOERROR"),
    ("Errno::ERFKILL", "Errno::NOERROR"),
    ("Errno::ESRMNT", "Errno::NOERROR"),
    ("Errno::ESTRPIPE", "Errno::NOERROR"),
    ("Errno::EUCLEAN", "Errno::NOERROR"),
    ("Errno::EUNATCH", "Errno::NOERROR"),
    ("Errno::EWOULDBLOCK", "Errno::EAGAIN"),
    ("Errno::EXFULL", "Errno::NOERROR"),
];

/// The Linux set, derived from `libc`'s `asm-generic` values -- the ones
/// x86_64, aarch64, riscv64 and loongarch64 share. It is not oracle-verified
/// on Linux, and the architectures that renumber errno
/// (mips, sparc, parisc, alpha) would need a table of their own.
#[cfg(not(target_vendor = "apple"))]
pub const ERRNO_CLASSES: &[ErrnoClass] = &[
    ErrnoClass {
        name: "Errno::E2BIG",
        errno: 7,
    },
    ErrnoClass {
        name: "Errno::EACCES",
        errno: 13,
    },
    ErrnoClass {
        name: "Errno::EADDRINUSE",
        errno: 98,
    },
    ErrnoClass {
        name: "Errno::EADDRNOTAVAIL",
        errno: 99,
    },
    ErrnoClass {
        name: "Errno::EADV",
        errno: 68,
    },
    ErrnoClass {
        name: "Errno::EAFNOSUPPORT",
        errno: 97,
    },
    ErrnoClass {
        name: "Errno::EAGAIN",
        errno: 11,
    },
    ErrnoClass {
        name: "Errno::EALREADY",
        errno: 114,
    },
    ErrnoClass {
        name: "Errno::EBADE",
        errno: 52,
    },
    ErrnoClass {
        name: "Errno::EBADF",
        errno: 9,
    },
    ErrnoClass {
        name: "Errno::EBADFD",
        errno: 77,
    },
    ErrnoClass {
        name: "Errno::EBADMSG",
        errno: 74,
    },
    ErrnoClass {
        name: "Errno::EBADR",
        errno: 53,
    },
    ErrnoClass {
        name: "Errno::EBADRQC",
        errno: 56,
    },
    ErrnoClass {
        name: "Errno::EBADSLT",
        errno: 57,
    },
    ErrnoClass {
        name: "Errno::EBFONT",
        errno: 59,
    },
    ErrnoClass {
        name: "Errno::EBUSY",
        errno: 16,
    },
    ErrnoClass {
        name: "Errno::ECANCELED",
        errno: 125,
    },
    ErrnoClass {
        name: "Errno::ECHILD",
        errno: 10,
    },
    ErrnoClass {
        name: "Errno::ECHRNG",
        errno: 44,
    },
    ErrnoClass {
        name: "Errno::ECOMM",
        errno: 70,
    },
    ErrnoClass {
        name: "Errno::ECONNABORTED",
        errno: 103,
    },
    ErrnoClass {
        name: "Errno::ECONNREFUSED",
        errno: 111,
    },
    ErrnoClass {
        name: "Errno::ECONNRESET",
        errno: 104,
    },
    ErrnoClass {
        name: "Errno::EDEADLK",
        errno: 35,
    },
    ErrnoClass {
        name: "Errno::EDESTADDRREQ",
        errno: 89,
    },
    ErrnoClass {
        name: "Errno::EDOM",
        errno: 33,
    },
    ErrnoClass {
        name: "Errno::EDOTDOT",
        errno: 73,
    },
    ErrnoClass {
        name: "Errno::EDQUOT",
        errno: 122,
    },
    ErrnoClass {
        name: "Errno::EEXIST",
        errno: 17,
    },
    ErrnoClass {
        name: "Errno::EFAULT",
        errno: 14,
    },
    ErrnoClass {
        name: "Errno::EFBIG",
        errno: 27,
    },
    ErrnoClass {
        name: "Errno::EHOSTDOWN",
        errno: 112,
    },
    ErrnoClass {
        name: "Errno::EHOSTUNREACH",
        errno: 113,
    },
    ErrnoClass {
        name: "Errno::EHWPOISON",
        errno: 133,
    },
    ErrnoClass {
        name: "Errno::EIDRM",
        errno: 43,
    },
    ErrnoClass {
        name: "Errno::EILSEQ",
        errno: 84,
    },
    ErrnoClass {
        name: "Errno::EINPROGRESS",
        errno: 115,
    },
    ErrnoClass {
        name: "Errno::EINTR",
        errno: 4,
    },
    ErrnoClass {
        name: "Errno::EINVAL",
        errno: 22,
    },
    ErrnoClass {
        name: "Errno::EIO",
        errno: 5,
    },
    ErrnoClass {
        name: "Errno::EISCONN",
        errno: 106,
    },
    ErrnoClass {
        name: "Errno::EISDIR",
        errno: 21,
    },
    ErrnoClass {
        name: "Errno::EISNAM",
        errno: 120,
    },
    ErrnoClass {
        name: "Errno::EKEYEXPIRED",
        errno: 127,
    },
    ErrnoClass {
        name: "Errno::EKEYREJECTED",
        errno: 129,
    },
    ErrnoClass {
        name: "Errno::EKEYREVOKED",
        errno: 128,
    },
    ErrnoClass {
        name: "Errno::EL2HLT",
        errno: 51,
    },
    ErrnoClass {
        name: "Errno::EL2NSYNC",
        errno: 45,
    },
    ErrnoClass {
        name: "Errno::EL3HLT",
        errno: 46,
    },
    ErrnoClass {
        name: "Errno::EL3RST",
        errno: 47,
    },
    ErrnoClass {
        name: "Errno::ELIBACC",
        errno: 79,
    },
    ErrnoClass {
        name: "Errno::ELIBBAD",
        errno: 80,
    },
    ErrnoClass {
        name: "Errno::ELIBEXEC",
        errno: 83,
    },
    ErrnoClass {
        name: "Errno::ELIBMAX",
        errno: 82,
    },
    ErrnoClass {
        name: "Errno::ELIBSCN",
        errno: 81,
    },
    ErrnoClass {
        name: "Errno::ELNRNG",
        errno: 48,
    },
    ErrnoClass {
        name: "Errno::ELOOP",
        errno: 40,
    },
    ErrnoClass {
        name: "Errno::EMEDIUMTYPE",
        errno: 124,
    },
    ErrnoClass {
        name: "Errno::EMFILE",
        errno: 24,
    },
    ErrnoClass {
        name: "Errno::EMLINK",
        errno: 31,
    },
    ErrnoClass {
        name: "Errno::EMSGSIZE",
        errno: 90,
    },
    ErrnoClass {
        name: "Errno::EMULTIHOP",
        errno: 72,
    },
    ErrnoClass {
        name: "Errno::ENAMETOOLONG",
        errno: 36,
    },
    ErrnoClass {
        name: "Errno::ENAVAIL",
        errno: 119,
    },
    ErrnoClass {
        name: "Errno::ENETDOWN",
        errno: 100,
    },
    ErrnoClass {
        name: "Errno::ENETRESET",
        errno: 102,
    },
    ErrnoClass {
        name: "Errno::ENETUNREACH",
        errno: 101,
    },
    ErrnoClass {
        name: "Errno::ENFILE",
        errno: 23,
    },
    ErrnoClass {
        name: "Errno::ENOANO",
        errno: 55,
    },
    ErrnoClass {
        name: "Errno::ENOBUFS",
        errno: 105,
    },
    ErrnoClass {
        name: "Errno::ENOCSI",
        errno: 50,
    },
    ErrnoClass {
        name: "Errno::ENODATA",
        errno: 61,
    },
    ErrnoClass {
        name: "Errno::ENODEV",
        errno: 19,
    },
    ErrnoClass {
        name: "Errno::ENOENT",
        errno: 2,
    },
    ErrnoClass {
        name: "Errno::ENOEXEC",
        errno: 8,
    },
    ErrnoClass {
        name: "Errno::ENOKEY",
        errno: 126,
    },
    ErrnoClass {
        name: "Errno::ENOLCK",
        errno: 37,
    },
    ErrnoClass {
        name: "Errno::ENOLINK",
        errno: 67,
    },
    ErrnoClass {
        name: "Errno::ENOMEDIUM",
        errno: 123,
    },
    ErrnoClass {
        name: "Errno::ENOMEM",
        errno: 12,
    },
    ErrnoClass {
        name: "Errno::ENOMSG",
        errno: 42,
    },
    ErrnoClass {
        name: "Errno::ENONET",
        errno: 64,
    },
    ErrnoClass {
        name: "Errno::ENOPKG",
        errno: 65,
    },
    ErrnoClass {
        name: "Errno::ENOPROTOOPT",
        errno: 92,
    },
    ErrnoClass {
        name: "Errno::ENOSPC",
        errno: 28,
    },
    ErrnoClass {
        name: "Errno::ENOSR",
        errno: 63,
    },
    ErrnoClass {
        name: "Errno::ENOSTR",
        errno: 60,
    },
    ErrnoClass {
        name: "Errno::ENOSYS",
        errno: 38,
    },
    ErrnoClass {
        name: "Errno::ENOTBLK",
        errno: 15,
    },
    ErrnoClass {
        name: "Errno::ENOTCONN",
        errno: 107,
    },
    ErrnoClass {
        name: "Errno::ENOTDIR",
        errno: 20,
    },
    ErrnoClass {
        name: "Errno::ENOTEMPTY",
        errno: 39,
    },
    ErrnoClass {
        name: "Errno::ENOTNAM",
        errno: 118,
    },
    ErrnoClass {
        name: "Errno::ENOTRECOVERABLE",
        errno: 131,
    },
    ErrnoClass {
        name: "Errno::ENOTSOCK",
        errno: 88,
    },
    ErrnoClass {
        name: "Errno::ENOTSUP",
        errno: 95,
    },
    ErrnoClass {
        name: "Errno::ENOTTY",
        errno: 25,
    },
    ErrnoClass {
        name: "Errno::ENOTUNIQ",
        errno: 76,
    },
    ErrnoClass {
        name: "Errno::ENXIO",
        errno: 6,
    },
    ErrnoClass {
        name: "Errno::EOVERFLOW",
        errno: 75,
    },
    ErrnoClass {
        name: "Errno::EOWNERDEAD",
        errno: 130,
    },
    ErrnoClass {
        name: "Errno::EPERM",
        errno: 1,
    },
    ErrnoClass {
        name: "Errno::EPFNOSUPPORT",
        errno: 96,
    },
    ErrnoClass {
        name: "Errno::EPIPE",
        errno: 32,
    },
    ErrnoClass {
        name: "Errno::EPROTO",
        errno: 71,
    },
    ErrnoClass {
        name: "Errno::EPROTONOSUPPORT",
        errno: 93,
    },
    ErrnoClass {
        name: "Errno::EPROTOTYPE",
        errno: 91,
    },
    ErrnoClass {
        name: "Errno::ERANGE",
        errno: 34,
    },
    ErrnoClass {
        name: "Errno::EREMCHG",
        errno: 78,
    },
    ErrnoClass {
        name: "Errno::EREMOTE",
        errno: 66,
    },
    ErrnoClass {
        name: "Errno::EREMOTEIO",
        errno: 121,
    },
    ErrnoClass {
        name: "Errno::ERESTART",
        errno: 85,
    },
    ErrnoClass {
        name: "Errno::ERFKILL",
        errno: 132,
    },
    ErrnoClass {
        name: "Errno::EROFS",
        errno: 30,
    },
    ErrnoClass {
        name: "Errno::ESHUTDOWN",
        errno: 108,
    },
    ErrnoClass {
        name: "Errno::ESOCKTNOSUPPORT",
        errno: 94,
    },
    ErrnoClass {
        name: "Errno::ESPIPE",
        errno: 29,
    },
    ErrnoClass {
        name: "Errno::ESRCH",
        errno: 3,
    },
    ErrnoClass {
        name: "Errno::ESRMNT",
        errno: 69,
    },
    ErrnoClass {
        name: "Errno::ESTALE",
        errno: 116,
    },
    ErrnoClass {
        name: "Errno::ESTRPIPE",
        errno: 86,
    },
    ErrnoClass {
        name: "Errno::ETIME",
        errno: 62,
    },
    ErrnoClass {
        name: "Errno::ETIMEDOUT",
        errno: 110,
    },
    ErrnoClass {
        name: "Errno::ETOOMANYREFS",
        errno: 109,
    },
    ErrnoClass {
        name: "Errno::ETXTBSY",
        errno: 26,
    },
    ErrnoClass {
        name: "Errno::EUCLEAN",
        errno: 117,
    },
    ErrnoClass {
        name: "Errno::EUNATCH",
        errno: 49,
    },
    ErrnoClass {
        name: "Errno::EUSERS",
        errno: 87,
    },
    ErrnoClass {
        name: "Errno::EXDEV",
        errno: 18,
    },
    ErrnoClass {
        name: "Errno::EXFULL",
        errno: 54,
    },
    ErrnoClass {
        name: "Errno::NOERROR",
        errno: 0,
    },
];

/// The second names on Linux. `ENOTSUP` and `EOPNOTSUPP` are one value there,
/// as are `EDEADLK` and `EDEADLOCK`; the rest are the BSD-only names, bound to
/// `Errno::NOERROR`.
#[cfg(not(target_vendor = "apple"))]
pub const ERRNO_ALIASES: &[(&str, &str)] = &[
    ("Errno::EAUTH", "Errno::NOERROR"),
    ("Errno::EBADARCH", "Errno::NOERROR"),
    ("Errno::EBADEXEC", "Errno::NOERROR"),
    ("Errno::EBADMACHO", "Errno::NOERROR"),
    ("Errno::EBADRPC", "Errno::NOERROR"),
    ("Errno::ECAPMODE", "Errno::NOERROR"),
    ("Errno::EDEADLOCK", "Errno::EDEADLK"),
    ("Errno::EDEVERR", "Errno::NOERROR"),
    ("Errno::EDOOFUS", "Errno::NOERROR"),
    ("Errno::EFTYPE", "Errno::NOERROR"),
    ("Errno::EIPSEC", "Errno::NOERROR"),
    ("Errno::ELAST", "Errno::NOERROR"),
    ("Errno::ENEEDAUTH", "Errno::NOERROR"),
    ("Errno::ENOATTR", "Errno::NOERROR"),
    ("Errno::ENOPOLICY", "Errno::NOERROR"),
    ("Errno::ENOTCAPABLE", "Errno::NOERROR"),
    ("Errno::EOPNOTSUPP", "Errno::ENOTSUP"),
    ("Errno::EPROCLIM", "Errno::NOERROR"),
    ("Errno::EPROCUNAVAIL", "Errno::NOERROR"),
    ("Errno::EPROGMISMATCH", "Errno::NOERROR"),
    ("Errno::EPROGUNAVAIL", "Errno::NOERROR"),
    ("Errno::EPWROFF", "Errno::NOERROR"),
    ("Errno::EQFULL", "Errno::NOERROR"),
    ("Errno::ERPCMISMATCH", "Errno::NOERROR"),
    ("Errno::ESHLIBVERS", "Errno::NOERROR"),
    ("Errno::EWOULDBLOCK", "Errno::EAGAIN"),
];
