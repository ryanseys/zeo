/*
 * `rb_sprintf` and everything that formats a message.
 *
 * MRI's format string is C's plus one extension, and the extension is the
 * whole reason this file exists. `PRIsVALUE` expands to `"li\v"` -- a
 * conversion that looks like `%li` followed by a literal vertical tab -- and
 * MRI's own `vsnprintf` copy treats that pair as "the argument is a VALUE,
 * print what `to_s` answers". A `%+` flag asks for `inspect` instead.
 *
 *     rb_raise(rb_eTypeError, "no %"PRIsVALUE" for %+"PRIsVALUE, name, obj);
 *
 * Handing that to the system `vsnprintf` prints the VALUE's bit pattern as a
 * decimal integer, so the message reads `no 4 for 8791234560` and the `\v`
 * lands in the output. Hundreds of the census's 398 `rb_raise` calls write
 * it. So the format is walked here, one conversion at a time: each is copied
 * out, its argument is `va_arg`ed at the type the spec names, and the pair is
 * handed to `snprintf` on its own. The VALUE conversions are answered by
 * calling back into Rust for the text.
 *
 * Walking rather than delegating is also what makes the type reads safe. A
 * `va_list` has to be read at exactly the type the caller pushed, and the
 * conversion spec is the only description of that -- so the spec is parsed
 * fully, length modifiers included, rather than assumed.
 */

#include <stdarg.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>

typedef unsigned long VALUE;
typedef unsigned long ID;

/* Implemented in Rust (crates/zeo-capi/src/format.rs). */
extern const char *zeo_cext_value_text(VALUE v, int inspect);
extern VALUE zeo_cext_str_new_len(const char *p, long len);
extern VALUE rb_enc_str_new(const char *p, long len, void *enc);
extern VALUE zeo_cext_str_cat_len(VALUE str, const char *p, long len);

/* MRI truncates a formatted message too; this is its ceiling for one. */
#define ZEO_FMT_MAX 4096
/* One conversion's spec: `%-+ 0#*.*llx` and a NUL is far inside this. */
#define ZEO_SPEC_MAX 64

/* The sentinel `PRIsVALUE` leaves after its conversion. */
#define ZEO_VALUE_MARK '\v'

/* How wide the integer argument is. The spec's length modifier is the only
 * thing that says, and reading a `long` where an `int` was pushed is exactly
 * the bug this file exists to avoid. */
enum zeo_len { L_INT, L_CHAR, L_SHORT, L_LONG, L_LLONG, L_SIZE, L_PTRDIFF, L_INTMAX };

struct zeo_out {
    char *buf;
    size_t cap;
    size_t len;
};

static void zeo_out_str(struct zeo_out *o, const char *s, size_t n)
{
    size_t room;

    if (o->len >= o->cap) {
        return;
    }
    room = o->cap - o->len - 1;
    if (n > room) {
        n = room;
    }
    memcpy(o->buf + o->len, s, n);
    o->len += n;
    o->buf[o->len] = '\0';
}

/* Format one conversion whose spec is `spec` and whose single argument has
 * already been read into the right union member. */
#define ZEO_EMIT(o, spec, val)                                          \
    do {                                                                \
        char zeo_tmp[ZEO_FMT_MAX];                                      \
        int zeo_n = snprintf(zeo_tmp, sizeof(zeo_tmp), (spec), (val));  \
        if (zeo_n > 0) {                                                \
            zeo_out_str((o), zeo_tmp, (size_t)zeo_n);                   \
        }                                                               \
    } while (0)

/*
 * Walk `fmt`, writing into `o`.
 *
 * Returns nothing: a format this cannot parse is copied through verbatim,
 * which is what MRI's own fallback does and is always better than dropping
 * the message an extension was trying to raise with.
 */
static void zeo_vformat(struct zeo_out *o, const char *fmt, va_list ap)
{
    const char *p = fmt;

    while (*p != '\0') {
        char spec[ZEO_SPEC_MAX];
        size_t sl = 0;
        int stars = 0;
        int star_w = 0, star_p = 0;
        int plus = 0;
        enum zeo_len len = L_INT;
        char conv;
        const char *run;

        if (*p != '%') {
            run = p;
            while (*p != '\0' && *p != '%') {
                p++;
            }
            zeo_out_str(o, run, (size_t)(p - run));
            continue;
        }

        /* `%%` is a literal, and consumes no argument. */
        if (p[1] == '%') {
            zeo_out_str(o, "%", 1);
            p += 2;
            continue;
        }

        spec[sl++] = *p++;
        /* Flags. */
        while (*p == '-' || *p == '+' || *p == ' ' || *p == '#' || *p == '0') {
            if (*p == '+') {
                plus = 1;
            }
            if (sl + 1 < ZEO_SPEC_MAX) {
                spec[sl++] = *p;
            }
            p++;
        }
        /* Width, then precision. `*` takes an `int` argument each. */
        while ((*p >= '0' && *p <= '9') || *p == '*') {
            if (*p == '*') {
                star_w = va_arg(ap, int);
                stars |= 1;
            }
            if (sl + 1 < ZEO_SPEC_MAX) {
                spec[sl++] = *p;
            }
            p++;
        }
        if (*p == '.') {
            if (sl + 1 < ZEO_SPEC_MAX) {
                spec[sl++] = *p;
            }
            p++;
            while ((*p >= '0' && *p <= '9') || *p == '*') {
                if (*p == '*') {
                    star_p = va_arg(ap, int);
                    stars |= 2;
                }
                if (sl + 1 < ZEO_SPEC_MAX) {
                    spec[sl++] = *p;
                }
                p++;
            }
        }
        /* Length modifiers. */
        for (;;) {
            if (p[0] == 'h' && p[1] == 'h') {
                len = L_CHAR;
            } else if (p[0] == 'h') {
                len = L_SHORT;
            } else if (p[0] == 'l' && p[1] == 'l') {
                len = L_LLONG;
            } else if (p[0] == 'l') {
                len = L_LONG;
            } else if (p[0] == 'z') {
                len = L_SIZE;
            } else if (p[0] == 't') {
                len = L_PTRDIFF;
            } else if (p[0] == 'j') {
                len = L_INTMAX;
            } else if (p[0] == 'q') {
                len = L_LLONG;
            } else {
                break;
            }
            if (sl + 1 < ZEO_SPEC_MAX) {
                spec[sl++] = *p;
            }
            p++;
            if (len == L_CHAR || len == L_LLONG) {
                if (sl + 1 < ZEO_SPEC_MAX) {
                    spec[sl++] = *p;
                }
                p++;
            }
            break;
        }

        conv = *p;
        if (conv == '\0') {
            /* A trailing `%`: copy what was collected and stop. */
            zeo_out_str(o, spec, sl);
            break;
        }
        if (sl + 2 < ZEO_SPEC_MAX) {
            spec[sl++] = conv;
        }
        spec[sl] = '\0';
        p++;

        /* MRI's extension: the conversion is followed by the sentinel, so
         * the argument is a VALUE and not the integer the spec claims. */
        if (*p == ZEO_VALUE_MARK && (conv == 'i' || conv == 'd' || conv == 'u')) {
            VALUE v = va_arg(ap, VALUE);
            const char *text = zeo_cext_value_text(v, plus);
            char sspec[ZEO_SPEC_MAX];

            p++;
            /* Reuse the flags and width, as `%s`. */
            memcpy(sspec, spec, sl + 1);
            sspec[sl - 1] = 's';
            if (text == NULL) {
                text = "";
            }
            if (stars == 3) {
                char tmp[ZEO_FMT_MAX];
                int n = snprintf(tmp, sizeof(tmp), sspec, star_w, star_p, text);
                if (n > 0) {
                    zeo_out_str(o, tmp, (size_t)n);
                }
            } else if (stars) {
                char tmp[ZEO_FMT_MAX];
                int n = snprintf(tmp, sizeof(tmp), sspec, stars == 1 ? star_w : star_p, text);
                if (n > 0) {
                    zeo_out_str(o, tmp, (size_t)n);
                }
            } else {
                ZEO_EMIT(o, sspec, text);
            }
            continue;
        }

        /* `*` already consumed its arguments above, and re-passing them
         * through the same spec is how `snprintf` gets them back. Only the
         * no-star case is common, so the starred ones are spelled out rather
         * than folded into a macro that would hide the argument count. */
        switch (conv) {
        case 'd': case 'i': case 'o': case 'u': case 'x': case 'X': {
            long long n;
            switch (len) {
            case L_CHAR:    n = (signed char)va_arg(ap, int); break;
            case L_SHORT:   n = (short)va_arg(ap, int); break;
            case L_LONG:    n = va_arg(ap, long); break;
            case L_LLONG:   n = va_arg(ap, long long); break;
            case L_SIZE:    n = (long long)va_arg(ap, size_t); break;
            case L_PTRDIFF: n = (long long)va_arg(ap, ptrdiff_t); break;
            case L_INTMAX:  n = va_arg(ap, long long); break;
            default:        n = va_arg(ap, int); break;
            }
            /* Re-spell the length as `ll`, so one `snprintf` call covers
             * every width without the spec and the read disagreeing. */
            {
                char wide[ZEO_SPEC_MAX];
                size_t i = 0, j = 0;
                for (; i < sl; i++) {
                    char c = spec[i];
                    if (c == 'h' || c == 'l' || c == 'z' || c == 't' || c == 'j' || c == 'q') {
                        continue;
                    }
                    if (i + 1 == sl && j + 2 < ZEO_SPEC_MAX) {
                        wide[j++] = 'l';
                        wide[j++] = 'l';
                    }
                    wide[j++] = c;
                }
                wide[j] = '\0';
                if (stars == 3) {
                    char tmp[ZEO_FMT_MAX];
                    int w = snprintf(tmp, sizeof(tmp), wide, star_w, star_p, n);
                    if (w > 0) { zeo_out_str(o, tmp, (size_t)w); }
                } else if (stars) {
                    char tmp[ZEO_FMT_MAX];
                    int w = snprintf(tmp, sizeof(tmp), wide, stars == 1 ? star_w : star_p, n);
                    if (w > 0) { zeo_out_str(o, tmp, (size_t)w); }
                } else {
                    ZEO_EMIT(o, wide, n);
                }
            }
            break;
        }
        case 'f': case 'F': case 'e': case 'E': case 'g': case 'G': case 'a': case 'A': {
            double d = va_arg(ap, double);
            if (stars == 3) {
                char tmp[ZEO_FMT_MAX];
                int w = snprintf(tmp, sizeof(tmp), spec, star_w, star_p, d);
                if (w > 0) { zeo_out_str(o, tmp, (size_t)w); }
            } else if (stars) {
                char tmp[ZEO_FMT_MAX];
                int w = snprintf(tmp, sizeof(tmp), spec, stars == 1 ? star_w : star_p, d);
                if (w > 0) { zeo_out_str(o, tmp, (size_t)w); }
            } else {
                ZEO_EMIT(o, spec, d);
            }
            break;
        }
        case 'c': {
            int c = va_arg(ap, int);
            ZEO_EMIT(o, spec, c);
            break;
        }
        case 's': {
            const char *s = va_arg(ap, const char *);
            if (s == NULL) {
                s = "(null)";
            }
            if (stars == 3) {
                char tmp[ZEO_FMT_MAX];
                int w = snprintf(tmp, sizeof(tmp), spec, star_w, star_p, s);
                if (w > 0) { zeo_out_str(o, tmp, (size_t)w); }
            } else if (stars) {
                char tmp[ZEO_FMT_MAX];
                int w = snprintf(tmp, sizeof(tmp), spec, stars == 1 ? star_w : star_p, s);
                if (w > 0) { zeo_out_str(o, tmp, (size_t)w); }
            } else {
                ZEO_EMIT(o, spec, s);
            }
            break;
        }
        case 'p': {
            void *v = va_arg(ap, void *);
            ZEO_EMIT(o, spec, v);
            break;
        }
        default:
            /* Not a conversion this knows. Copy it through rather than
             * guessing at an argument that may not be there. */
            zeo_out_str(o, spec, sl);
            break;
        }
    }
}

/* The one entry every formatting function here goes through. Answers the
 * length written, not counting the NUL, as `vsnprintf` does. */
int zeo_cext_vsnprintf(char *buf, size_t cap, const char *fmt, va_list ap)
{
    struct zeo_out o;

    if (cap == 0) {
        return 0;
    }
    o.buf = buf;
    o.cap = cap;
    o.len = 0;
    o.buf[0] = '\0';
    if (fmt != NULL) {
        zeo_vformat(&o, fmt, ap);
    }
    return (int)o.len;
}

int ruby_vsnprintf(char *buf, size_t cap, const char *fmt, va_list ap)
{
    return zeo_cext_vsnprintf(buf, cap, fmt, ap);
}

int ruby_snprintf(char *buf, size_t cap, const char *fmt, ...)
{
    va_list ap;
    int n;

    va_start(ap, fmt);
    n = zeo_cext_vsnprintf(buf, cap, fmt, ap);
    va_end(ap);
    return n;
}

VALUE rb_vsprintf(const char *fmt, va_list ap)
{
    char msg[ZEO_FMT_MAX];
    int n = zeo_cext_vsnprintf(msg, sizeof(msg), fmt, ap);

    return zeo_cext_str_new_len(msg, (long)n);
}

VALUE rb_sprintf(const char *fmt, ...)
{
    char msg[ZEO_FMT_MAX];
    va_list ap;
    int n;

    va_start(ap, fmt);
    n = zeo_cext_vsnprintf(msg, sizeof(msg), fmt, ap);
    va_end(ap);
    return zeo_cext_str_new_len(msg, (long)n);
}

/*
 * `rb_enc_vsprintf` / `rb_enc_sprintf`: the same formatting, with the caller
 * naming the answer's encoding instead of taking the default.
 *
 * `rb_encoding *` is a `void *` here because this file includes no ruby
 * header; it is a pointer either way, so the ABI is the same one the
 * extension's own prototype describes. What the pointer MEANS is zeo's
 * business, and `rb_enc_str_new` is where it is read.
 */
VALUE rb_enc_vsprintf(void *enc, const char *fmt, va_list ap)
{
    char msg[ZEO_FMT_MAX];
    int n = zeo_cext_vsnprintf(msg, sizeof(msg), fmt, ap);

    return rb_enc_str_new(msg, (long)n, enc);
}

VALUE rb_enc_sprintf(void *enc, const char *fmt, ...)
{
    char msg[ZEO_FMT_MAX];
    va_list ap;
    int n;

    va_start(ap, fmt);
    n = zeo_cext_vsnprintf(msg, sizeof(msg), fmt, ap);
    va_end(ap);
    return rb_enc_str_new(msg, (long)n, enc);
}

VALUE rb_str_vcatf(VALUE str, const char *fmt, va_list ap)
{
    char msg[ZEO_FMT_MAX];
    int n = zeo_cext_vsnprintf(msg, sizeof(msg), fmt, ap);

    return zeo_cext_str_cat_len(str, msg, (long)n);
}

VALUE rb_str_catf(VALUE str, const char *fmt, ...)
{
    char msg[ZEO_FMT_MAX];
    va_list ap;
    int n;

    va_start(ap, fmt);
    n = zeo_cext_vsnprintf(msg, sizeof(msg), fmt, ap);
    va_end(ap);
    return zeo_cext_str_cat_len(str, msg, (long)n);
}
