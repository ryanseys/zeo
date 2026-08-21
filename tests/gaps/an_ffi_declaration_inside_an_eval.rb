# An `FFI::Library` declaration written inside a run-time `eval` is
# DECLINED, loudly. zeo resolves the whole FFI surface at compile time --
# `extend FFI::Library`, `ffi_lib`, `attach_function`, and a
# `class < FFI::Struct`'s `layout` are directives the whole-program walk
# consumes into one `FfiCall` tree in `.rodata` -- and a snippet has no
# such walk. Ruby attaches at run time and answers.
#
# What it used to do instead is why this is written down: the directives
# were consumed by the snippet's own lowering, attached nothing, and then
# the `attach_function` call reached a module that had never been
# extended -- a `NoMethodError` reading as though the name were
# misspelled. `FFI::Library` now carries `ffi_lib`/`attach_function`/
# `attach_variable` rows that say what actually happened, and a snippet
# declares no FFI library at all (`lower::defs`' `is_ffi` gate).
#
# The fix shape, when it is worth it: the run-time libffi tier is
# already the ONLY tier the CLIF backend uses (`clif/ffi.rs` sends every
# call through `zeo_rt_ffi_call_fixed`), so an eval'd declaration needs
# no new engine -- it needs the DECLARATION to be buildable at run time:
# a `FfiCallC` tree minted from values rather than baked into `.rodata`,
# and `attach_function` installing a body that carries it. That is a
# real feature, not a widening of this compile.

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
