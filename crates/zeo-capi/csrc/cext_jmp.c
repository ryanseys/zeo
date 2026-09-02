/*
 * The non-local exit a C extension expects.
 *
 * `rb_raise` does not return. Neither does `rb_throw`, `rb_jump_tag`, or a
 * `break` out of `rb_block_call`. In MRI all of them are a `longjmp` to the
 * innermost `rb_protect`-shaped frame, and an extension is written on that
 * assumption: it allocates with `ruby_xmalloc`, wraps the risky part in
 * `rb_ensure`, and expects the stack between to be discarded without running
 * anything.
 *
 * zeo signals errors with `Result<_, Signal>`, which unwinds by returning.
 * The two cannot be bridged in Rust: a `longjmp` past a live Rust frame skips
 * its destructors, which is undefined behaviour and not merely a leak. So the
 * `jmp_buf` chain lives here, in C, and the rule is absolute:
 *
 *     THE LONGJMP ONLY EVER UNWINDS C FRAMES OF THE EXTENSION.
 *
 * `zeo_cext_call_protected` is the only place a `setjmp` happens, and the
 * frame it establishes sits immediately below the extension's own C frames
 * and immediately above the Rust that called in. Every Rust entry that C can
 * reach is written through the `cext_fn!` macro, which stores the `Signal`
 * and calls `zeo_cext_jump_tag` only after its Rust body has fully returned
 * and no destructor is live.
 *
 * The chain head lives on the Rust side (`cext::jmp`), because it has to
 * follow a fiber's stack rather than the OS thread's.
 */

#include <setjmp.h>
#include <stdio.h>
#include <stdlib.h>

typedef struct zeo_cext_jmp {
    struct zeo_cext_jmp *prev;
    jmp_buf buf;
} zeo_cext_jmp;

/* Implemented in Rust. */
extern zeo_cext_jmp *zeo_cext_jmp_head(void);
extern void zeo_cext_jmp_set_head(zeo_cext_jmp *head);

/*
 * Run `body(arg)` with a landing pad in place.
 *
 * Answers 0 when the body returned, and the jump tag when it did not. `*out`
 * is written only on the returning path: after a longjmp the body's return
 * value never existed, and leaving `*out` alone is what stops a caller
 * reading an uninitialised one.
 *
 * `frame` is on this function's own stack, so it is still addressable when
 * the longjmp lands back in `setjmp` -- which is what makes restoring `prev`
 * correct on both paths.
 */
int zeo_cext_call_protected(void *(*body)(void *), void *arg, void **out)
{
    zeo_cext_jmp frame;
    int tag;

    frame.prev = zeo_cext_jmp_head();
    zeo_cext_jmp_set_head(&frame);

    /* `setjmp`, not `sigsetjmp`: MRI does the same, and saving the signal
     * mask on every protected call is a syscall an extension never asked
     * for. */
    tag = setjmp(frame.buf);
    if (tag == 0) {
        void *r = body(arg);
        if (out != NULL) {
            *out = r;
        }
    }

    zeo_cext_jmp_set_head(frame.prev);
    return tag;
}

/*
 * Leave the innermost protected frame with `tag`.
 *
 * A null head means C is unwinding with nothing to unwind to, which can only
 * happen if a Rust entry point was reached without `zeo_cext_call_protected`
 * around it. That is a bug in zeo, not in the extension, and continuing would
 * corrupt the stack -- so it aborts, and says which of the two it is.
 */
void zeo_cext_jump_tag(int tag)
{
    zeo_cext_jmp *head = zeo_cext_jmp_head();
    if (head == NULL) {
        fputs("zeo: a C extension raised with no protected frame open; "
              "this is a bug in zeo's cext entry points, not in the gem\n",
              stderr);
        abort();
    }
    /* `longjmp(buf, 0)` is defined to arrive as 1, which would read as "the
     * body returned". Never hand it a zero. */
    longjmp(head->buf, tag == 0 ? 1 : tag);
}

/* `sizeof` for the Rust side's layout assert. A `jmp_buf` is
 * platform-defined, so Rust cannot know this number without asking. */
size_t zeo_cext_jmp_frame_size(void)
{
    return sizeof(zeo_cext_jmp);
}
