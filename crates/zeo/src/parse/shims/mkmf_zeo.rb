# frozen_string_literal: true

# The delta zeo maintains over the vendored `mkmf.rb`, kept beside it rather
# than patched into it so the vendored copy stays byte-identical to ruby's.
#
# `have_func` does not read headers -- it COMPILES AND LINKS a `conftest`
# executable naming the symbol. CRuby links it against `$LIBRUBYARG`
# (`-lruby.3.x`) and the symbol resolves. zeo has no shared runtime library:
# the runtime lives inside the compiled program, and an extension resolves
# against the host binary's export table when it is `dlopen`ed. So
# `LIBRUBYARG` is empty, conftest links against nothing, and every probe used
# to answer "no".
#
# That is not a small wrong answer. Every C extension uses these probes to
# choose between a modern path and a compatibility fallback, so every gem
# compiled its oldest branch. Three in the Gemfile then failed to build on the
# collision that follows -- json's `static rb_hash_bulk_insert`, io-console's
# `static rb_io_closed_p`, strscan's `static rb_reg_onig_match` -- each a
# fallback definition clashing with zeo's real, non-static one.
#
# THE HEADERS ARE THE ANSWER HERE. zeo's headers declare exactly the rows
# `cext/api.rs` carries -- implemented, stubbed and refused alike, all three
# being real exported symbols -- and a row zeo does not have is not declared
# at all. So "declared in these headers" and "resolves at load" are the same
# question, and a compile answers it without a linker.
#
# ONLY THE ADDRESS-TAKING FORM CAN ANSWER IT. `try_func` tries two programs:
# one that takes the function's address, and one that calls it. The second
# emits its own `extern void f();` first (mkmf.rb:893), so it compiles no
# matter what the headers say and only the link could ever have rejected it.
# The first has no such declaration, so it fails to compile on an undeclared
# name -- which is precisely the question. This runs that one, and does not
# fall through to the other.
#
# Probes that genuinely need a linker are untouched: `have_library`,
# `find_library` and `have_devel?` look for SYSTEM libraries, which are there
# to link against.
module MakeMakefile
  module ZeoProbes
    # `&var` and `f(args)` forms carry no address-taking program, so they keep
    # upstream's behaviour; `nil` is the "does the toolchain work" probe.
    def try_func(func, libs, headers = nil, opt = "", &b)
      return super if func.nil? || func.start_with?("&") || func.end_with?(")")
      try_compile(<<~SRC, opt, &b)
        #{cpp_include(headers)}
        /*top*/
        extern int t(void);
        int t(void) { void ((*volatile p)()); p = (void ((*)()))#{func}; return !p; }
      SRC
    end
  end
  prepend ZeoProbes
end
