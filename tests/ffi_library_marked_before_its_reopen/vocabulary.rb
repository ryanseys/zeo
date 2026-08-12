# A REOPEN of the library module. Nothing here says `extend FFI::Library`, so
# whether these are FFI directives at all depends on the enclosing file having
# been read first.
module Curl
  Status = enum(:status, status_codes)
  typedef :long, :ticks
end
