#@ backend: jit
# An FFI type declared in one compile can be NAMED by the next.
#
# A field type has to resolve to a byte width and an OFFSET at lowering time,
# and the tables that resolve it (`Hir::ffi_types`,
# `Hir::ffi_struct_layouts`) belong to ONE compile. A snippet is its own
# compile, so it had never heard of a `Pt` an earlier snippet -- or the
# program itself -- declared, even though the CLASS is right there at run
# time and `Pt.size` answers.
#
# `ffi_vocab` is the process-wide vocabulary every compile publishes into and
# a SNIPPET's compile seeds from. Sound because it is pure compile-time
# DESCRIPTION -- names to widths, offsets and field lists -- with no reference
# to any one arena. A whole-program compile deliberately does not seed: a
# program is compiled once, before anything has run, and letting one
# program's declarations reach another's would make a compile depend on what
# else the process had done.

require "ffi"

# A snippet's struct, named by the NEXT snippet.
eval <<~SRC
  class Pt < FFI::Struct
    layout :x, :int, :y, :int
  end
SRC
eval <<~SRC
  class Outer < FFI::Struct
    layout :head, :int, :pt, Pt
  end
SRC
o = Outer.new
o[:pt][:x] = 11
p [Outer.size, o[:pt][:x]]

# The PROGRAM's struct, named by a snippet -- the same limit the other way.
class Direct < FFI::Struct
  layout :a, :int, :b, :int
end
eval <<~SRC
  class Wrap < FFI::Struct
    layout :head, :int, :d, Direct
  end
SRC
w = Wrap.new
w[:d][:b] = 7
p [Wrap.size, w[:d][:b], Direct.size]

# Two levels of snippet-declared struct.
eval "class Two < FFI::Struct; layout :a, :int, :b, :int; end"
eval "class Three < FFI::Struct; layout :t, Two, :n, :int; end"
t = Three.new
t[:t][:a] = 3
p [Three.size, t[:t][:a]]

# A `.size` on an earlier snippet's struct is a compile-time constant there
# too.
eval "class Sized < FFI::Struct; layout :pad, [:char, Pt.size]; end"
p Sized.size
__END__
[12, 11]
[12, 7, 8]
[12, 3]
8
