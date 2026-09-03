# A `class < FFI::Struct` written inside a run-time `eval` declares its
# layout for real.
#
# A struct's `layout` is a compile-time directive: `lower::ffi` computes
# every field offset from the declared C types and REPLACES the directive,
# in place, with the accessors `synthesize_ffi_struct` writes. That is what
# a class body inside a snippet has to survive -- such a body runs as one
# more `class_eval` of its own SOURCE TEXT, recovered from the statements'
# spans, and a synthesized statement's span points into a source of
# analyze's own making. Registering that synthesized text as a file of its
# own is what makes it recoverable.
#
# The limit this cannot reach is the last section: the FFI type vocabulary
# is per-COMPILE, so a struct declared in one snippet cannot be NAMED as a
# field type by the next. See
# `tests/gaps/an_ffi_type_declared_in_one_snippet.rb`.
require "ffi"

def show
  yield
rescue Exception => e
  puts "#{e.class}: #{e.message.lines.first.to_s.strip[0, 60]}"
end

eval <<~SRC
  class Pt < FFI::Struct
    layout :x, :int, :y, :int

    def to_a = [self[:x], self[:y]]
  end
SRC
show do
  s = Pt.new
  s[:x] = 3
  s[:y] = 4
  p [Pt.size, Pt.offset_of(:y), s.to_a, Pt.members]
end

# Every storage shape a field can take, so the offsets are the C ones.
eval <<~SRC
  class Mixed < FFI::Struct
    layout :flag, :bool,
           :name, :string,
           :buf,  [:char, 8],
           :ptr,  :pointer
  end
SRC
show do
  m = Mixed.new
  m[:flag] = true
  p [Mixed.size, Mixed.offset_of(:ptr), m[:flag], m[:ptr].null?]
end

# A union is the same synthesis with every field at offset 0.
eval <<~SRC
  class U < FFI::Union
    layout :i, :int, :f, :double
  end
SRC
show { p [U.size, U.offset_of(:f)] }

# A struct named as another struct's field type, both in ONE snippet.
show do
  eval <<~SRC
    class P2 < FFI::Struct
      layout :x, :int, :y, :int
    end
    class O2 < FFI::Struct
      layout :head, :int, :pt, P2
    end
  SRC
  o = O2.new
  o[:pt][:x] = 11
  p [O2.size, o[:pt][:x]]
end

# A struct and a library declared together.
show do
  eval <<~SRC
    class Rec < FFI::Struct
      layout :sec, :long, :usec, :int
    end
    module TimeLib
      extend FFI::Library
      ffi_lib FFI::Library::LIBC
      attach_function :abs3, :abs, [:int], :int
    end
  SRC
  p [Rec.size, TimeLib.abs3(-5)]
end
__END__
[8, 4, [3, 4], [:x, :y]]
[32, 24, true, true]
[8, 0]
[12, 11]
[16, 5]
