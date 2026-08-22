# An FFI type declared in ONE snippet cannot be NAMED by the next.
#
# `Pt` here is an ordinary Ruby class, and after the first `eval` it exists
# -- `Pt.size` answers, `Pt.new` works. What does not carry over is the
# COMPILER's knowledge of it: a field type has to resolve to a byte width
# and an offset at lowering time, and the vocabulary that resolves it
# (`Hir::ffi_struct_layouts`, `Hir::ffi_types`) belongs to one compile. A
# snippet is its own compile, so the second one has never heard of `Pt`.
#
# The same limit runs the other way: a struct the compiled PROGRAM declared
# is unnameable from a snippet, because a program's layouts are compile-time
# data with no run-time form.
#
# Two snippets that declare both classes together do work -- see
# `tests/an_ffi_struct_declared_inside_an_eval.rb`, whose fourth section is
# exactly this shape inside one `eval`.
#
# The fix is a process-wide FFI vocabulary the eval compiler seeds from and
# merges back into, which also needs the program's own layouts to have a
# run-time form. That is a feature, not a widening of this compile.
#
# Oracle: the second declaration resolves `Pt` like any other constant.
require "ffi"

eval <<~SRC
  class Pt < FFI::Struct
    layout :x, :int, :y, :int
  end
SRC

begin
  eval <<~SRC
    class Outer < FFI::Struct
      layout :head, :int, :pt, Pt
    end
  SRC
  o = Outer.new
  o[:pt][:x] = 11
  p [Outer.size, o[:pt][:x]]
rescue Exception => e
  puts "#{e.class}: #{e.message.lines.first.to_s.strip[0, 60]}"
end
