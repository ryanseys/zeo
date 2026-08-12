# A POSIX typedef whose width differs between zeo's targets (mode_t is u16
# on macOS, u32 on glibc) is legal in argument/return position -- the
# generated code spells the TARGET's own libc alias, so rustc supplies the
# real width. A struct layout still rejects it: a field width that shifts
# by target would silently shift every later offset.
require "ffi"
require "tempfile"

module C
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :chmod, [:string, :mode_t], :int
  attach_function :clock, [], :clock_t
end

f = Tempfile.new("zeo-mode")
puts C.chmod(f.path, 0o644)
puts format("%o", File.stat(f.path).mode & 0o7777)
puts C.chmod(f.path, 0o600)
puts format("%o", File.stat(f.path).mode & 0o7777)
puts C.clock.is_a?(Integer)
f.close!
