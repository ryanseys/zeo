# A REOPEN of the library module. Nothing here says `extend FFI::Library`, so
# whether these are FFI directives at all depends on the enclosing file having
# been read first.
module Curl
  Status = enum(:status, status_codes)
  typedef :long, :ticks

  # The mirror image: a struct the REQUIRING file declared, taken by value.
  class Frame < FFI::Struct
    layout :head, Stamp.by_value, :n, :int
  end

  attach_function :measure, :llabs, [Stamp.by_value], :long
end
