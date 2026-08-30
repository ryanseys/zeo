# frozen_string_literal: true

# The delta zeo maintains over the vendored `mkmf.rb`, kept beside it rather
# than patched into it so the vendored copy stays byte-identical to ruby's.
#
# `have_func` does not read headers -- it COMPILES AND LINKS a `conftest`
# executable naming the symbol. CRuby links it against `$LIBRUBYARG`
# (`-lruby.3.x`) and the symbol resolves. zeo has no shared runtime library:
# the runtime lives inside the compiled program, and an extension resolves
# against the host binary's export table when it is `dlopen`ed. So
# `LIBRUBYARG` is empty, conftest links against nothing, and every probe for a
# ruby symbol used to answer "no".
#
# That is not a small wrong answer. Every C extension uses these probes to
# choose between a modern path and a compatibility fallback, so every gem
# compiled its oldest branch. Three in the Gemfile then failed to build on the
# collision that follows -- json's `static rb_hash_bulk_insert`, io-console's
# `static rb_io_closed_p`, strscan's `static rb_reg_onig_match` -- each a
# fallback definition clashing with zeo's real, non-static one.
#
# THE HEADERS ARE THE ANSWER FOR A RUBY SYMBOL. zeo's headers declare exactly
# the rows `cext/api.rs` carries -- implemented, stubbed and refused alike, all
# three being real exported symbols -- and a row zeo does not have is not
# declared at all. So "declared in these headers" and "resolves at load" are
# the same question, and a compile answers it without a linker.
#
# THE LINKER IS STILL THE ANSWER FOR A SYSTEM SYMBOL, and that is why this
# tries the compile FIRST and falls through rather than replacing the probe.
# `openlog` and `dlopen` are declared in `syslog.h` and `dlfcn.h`, which
# `ruby.h` never includes, so no compile against zeo's headers can see them --
# yet they resolve at link against libSystem, and syslog and fiddle both abort
# their `extconf.rb` on a "no". Refusing to fall through cost those two gems
# their build.
#
# The fallthrough is SAFE for the ruby symbols this exists to protect: a
# `rb_*` row zeo does not have is declared by no header AND exported by no
# system library, so the compile fails and the link fails after it. The two
# probes together are a three-way answer:
#
#   declared in zeo's headers  -> zeo has the row          -> yes
#   else, links against libc   -> a system function        -> yes
#   else                       -> neither                  -> no
module MakeMakefile
  module ZeoProbes
    def try_func(func, libs, headers = nil, opt = "", &b)
      src = zeo_probe_source(func, headers)
      return super unless src
      try_compile(src, opt, &b) || super
    end

    private

    # The compile-only twin of the program upstream's `try_func` would LINK,
    # per symbol shape (mkmf.rb:849-895). `nil` is the "does the toolchain
    # work at all" probe and names no symbol, so it has no twin and belongs to
    # the linker.
    def zeo_probe_source(func, headers)
      return nil if func.nil?
      headers = cpp_include(headers)
      case func
      when /\A&/
        # `&var`: upstream takes its address through a `const volatile void *`.
        <<~SRC
          #{headers}
          /*top*/
          extern int t(void);
          int t(void) { const volatile void *volatile p; p = (const volatile void *)#{func}; return !p; }
        SRC
      when /\)\z/
        # `f(args)`: upstream emits NO `extern` for this shape, so its program
        # already depends on the header declaration -- the only thing zeo drops
        # is the link. `""` in the argument list stands for a writable buffer,
        # which upstream substitutes for a local; do the same or the call will
        # not compile.
        strvars = []
        call = func.gsub(/""/) do
          v = "s#{strvars.size + 1}"
          strvars << v
          v
        end
        prepare = String.new
        unless strvars.empty?
          prepare << "char " << strvars.map { |v| %[#{v}[1024] = ""] }.join(", ") << "; "
        end
        <<~SRC
          #{headers}
          /*top*/
          extern int t(void);
          int t(void) { #{prepare}#{call}; return 0; }
        SRC
      else
        # A bare name: take its address. Upstream's OTHER program for this
        # shape declares `extern void #{func}();` itself and so compiles
        # whatever the headers say -- which is why only this one asks the
        # question zeo needs answered.
        <<~SRC
          #{headers}
          /*top*/
          extern int t(void);
          int t(void) { void ((*volatile p)()); p = (void ((*)()))#{func}; return !p; }
        SRC
      end
    end
  end
  prepend ZeoProbes
end
