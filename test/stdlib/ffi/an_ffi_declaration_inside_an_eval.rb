# An `FFI::Library` declaration written inside a run-time `eval` attaches
# for real.
#
# zeo resolves the whole FFI surface at compile time -- `extend
# FFI::Library`, `ffi_lib`, `attach_function` and a `class < FFI::Struct`'s
# `layout` are directives the whole-program walk consumes into one
# `FfiCall` tree in `.rodata` -- and a SNIPPET has no such walk. So a
# snippet's directives stay ordinary calls, and `FFI::Library`'s rows carry
# the run-time tier: `DynamicLibrary` opens the library, `dlsym` resolves
# the symbol, and `FFI::Function` (or `VariadicInvoker`) is the callable
# `attach_function` installs. The engine is the same libffi both tiers end
# at -- what the run time adds is a DECLARATION built from values rather
# than baked into `.rodata`.
#
# What it used to do instead is why this is written down: the directives
# were consumed by the snippet's own lowering, attached nothing, and then
# the `attach_function` call reached a module that had never been
# extended -- a `NoMethodError` reading as though the name were
# misspelled. See `an_ffi_declaration_attaches_at_run_time.rb` for the
# whole surface.

require "ffi"

module EvalAttached
end

SRC = [
  "module EvalAttached",
  "  extend FFI::Library",
  "  ffi_lib FFI::Library::LIBC",
  "  attach_function :abs, [:int], :int",
  "end",
].join("\n")

begin
  eval(SRC)
  p EvalAttached.abs(-7)
rescue Exception => e
  puts "#{e.class}: #{e.message.lines.first.strip}"
end
__END__
7
