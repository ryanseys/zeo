# A type position in an FFI declaration takes more than a literal symbol.
#
#  - `Struct.by_ref` is a POINTER to the struct, which is what the C prototype
#    receives; the layout never enters the call, so nothing about it is needed
#    to make one. get_process_mem passes its `TaskInfo` that way.
#  - an anonymous `Tag = enum(...)` is named by the constant it is assigned to,
#    not by a `:tag` argument, and is then written as that constant wherever a
#    type goes. sassc and google-protobuf declare every enum this way.
require "ffi"

module Libc
  extend FFI::Library
  ffi_lib FFI::Library::LIBC

  # Members auto-increment from 0, exactly as the `:tag` form's do.
  Whence = enum(:set, :cur, :end)

  # ... and an explicit value still restarts the run.
  Signal = enum(:hup, 1, :int, 2, :quit)

  typedef Whence, :whence_t

  attach_function :abs, [:int], :int
  # The same C `abs`, declared to take the enum. An enum IS an `int` at the
  # ABI -- that is the whole point of one: the names are ruby-side, the call
  # passes the number.
  attach_function :whence_abs, :abs, [:whence_t], :int
  attach_function :signal_abs, :abs, [Signal], :int
end

p Libc.abs(-7)
p Libc.whence_abs(-3)
p Libc.whence_abs(:end)
p Libc.signal_abs(:quit)

module Timing
  extend FFI::Library
  ffi_lib FFI::Library::LIBC

  class TimeVal < FFI::Struct
    layout :tv_sec, :int64,
           :tv_usec, :int64
  end

  attach_function :gettimeofday, [TimeVal.by_ref, :pointer], :int
end

tv = Timing::TimeVal.new
p Timing.gettimeofday(tv, nil)
p tv[:tv_sec] > 1_700_000_000
p Timing::TimeVal.members

# A struct field may be an enum too. In memory it is an `int`; what comes back
# out of it is the member's SYMBOL, and either spelling goes in.
module Shapes
  extend FFI::Library

  Kind = enum(:round, :square, :odd)

  class Tagged < FFI::Struct
    layout :kind, Kind,
           :n, :int32
  end

  # `FFI::Union` is the same declaration with every member at offset 0, and is
  # as wide as its widest member. A type declared in the library module reaches
  # the struct classes nested inside it.
  class Either < FFI::Union
    layout :as_int, :int32,
           :as_double, :double
  end
end

t = Shapes::Tagged.new
t[:kind] = 2
p t[:kind]
t[:kind] = :square
p t[:kind]
t[:n] = 9
p t[:n]
p Shapes::Tagged.size
p Shapes::Tagged.offset_of(:n)

e = Shapes::Either.new
e[:as_double] = 1.5
p e[:as_double]
p Shapes::Either.size
p Shapes::Either.offset_of(:as_double)

# A `:bool` field is one byte and reads back as true/false; a `:string` field
# is a `char *`, read through the pointer and NOT writable -- there would be
# nowhere to keep the bytes alive, so ruby refuses.
class Row < FFI::Struct
  layout :t, :int32,
         :flag, :bool,
         :name, :string,
         :n, :int32
end

p Row.size
p [Row.offset_of(:flag), Row.offset_of(:name), Row.offset_of(:n)]

r = Row.new
p r[:flag]
r[:flag] = true
p r[:flag]
r[:flag] = false
p r[:flag]
p r[:name]

begin
  r[:name] = "hi"
rescue ArgumentError => err
  p err.message
end
