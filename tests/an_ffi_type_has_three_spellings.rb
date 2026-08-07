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
