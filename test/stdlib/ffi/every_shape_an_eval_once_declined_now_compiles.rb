# There is no second evaluator to hand a declined shape to, so a shape
# the compiler cannot lower raises `NotImplementedError` naming itself.
# This test used to pin one such message -- and it had to be re-pointed
# every time a shape closed, which is a test that rots into a lie. It
# pins the CLOSURES instead: every shape a snippet once declined, each
# answering what ruby answers.
#@ ruby: -W:no-experimental

require "ffi"
# an `FFI::Library` declaration: the directives stay ordinary calls
# in a snippet, and the module's own rows attach at run time.
eval <<~SRC
  module EvalLib
    extend FFI::Library
    ffi_lib FFI::Library::LIBC
    attach_function :abs, [:int], :int
  end
SRC
p EvalLib.abs(-3)
# a `class < FFI::Struct`: its `layout` is replaced in place by
# synthesized accessors, whose own source is the body text that runs.
eval "class EvalStruct < FFI::Struct; layout :a, :int, :b, :int; end"
p EvalStruct.size
# `refine`/`using` at run time. (A `Ruby::Box` closed with them and
# is pinned by `test/lang/boxes/a_snippet_mints_no_compile_time_box.rb`,
# which the golden harness runs with `RUBY_BOX=1`.)
eval "module EvalRef; refine(String) { def shout = upcase + '!' }; end"
p eval("using EvalRef; 'hi'.shout")
__END__
3
8
"HI!"
