# `require "fileutils"` from inside an `eval` raises where the same require at
# the top level succeeds:
#
#   private method 'public' called for class #<Class:FileUtils::Verbose>
#
# The file is CRuby's own `fileutils.rb`, unmodified, and the construct is at
# its line 2650:
#
#   module Verbose
#     include FileUtils
#     names = ::FileUtils.collect_method(:verbose)
#     names.each { |name| module_eval("def #{name}(*args, **options) ... end") }
#     private(*names)
#     extend self
#     class << self
#       public(*::FileUtils::METHODS)
#     end
#   end
#
# Two facts locate it. The same `require` compiled STATICALLY works, in both
# the JIT and an AOT binary -- so the unit itself lowers correctly. And the
# construct alone, written out by hand inside an `eval`, also works: a module
# with a private instance method, `extend self`, and `class << self;
# public(:a); end` answers correctly. What fails is the two together, so the
# trigger is something the smaller shape does not carry -- `include` of a
# large module, or the `module_eval`-defined names, in a unit the RUN TIME
# compiled rather than the compile did.
#
# This is the recurring shape zeo's notes keep finding, one layer further in:
# a visibility verb the run time owns, applied against a set the compile-time
# view of the unit assembled. `private_constant` and `K.prepend` were the same
# bug at the top level and are fixed; this is the eval seam's copy of it.
#
# It BLOCKS DOGFOODING. A ruby tool compiled to a binary with `zeo -o`
# but its entry point unshifts a computed path onto `$LOAD_PATH`, so zeo
# cannot resolve those requires statically and defers each one to a run-time
# compile. The first library that requires `fileutils` then dies at load, and
# no zeo-compiled tool starts.

eval(%q{require "fileutils"})
puts FileUtils::Verbose.name
puts FileUtils::Verbose.singleton_class.public_method_defined?(:cp)
