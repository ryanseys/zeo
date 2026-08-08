# An ARRAY argument to `ffi_lib` is a list of ALTERNATIVE spellings of ONE
# library, and ruby loads the first that dlopens. (Separate arguments are
# separate libraries, all required -- a different thing.) zeo has to name one
# library at compile time, so it takes the first alternative it can DECIDE.
#
# It used to accept only a bare string or `FFI::Library::LIBC`, which refused
# every gem that writes an alternatives list, a symbol, or the `ffi` gem's own
# `FFI.library_name` helper -- 317 corpus rows, the largest single lowering
# diagnostic left after the singleton clusters.
require "ffi"

# A candidate LIST. The bare name comes first and the versioned sonames after
# it, which is why first-decidable is the right pick: rbnacl writes
# `["sodium", "libsodium.so.18", "libsodium.so.23", "libsodium.so.26"]`.
module ListForm
  extend FFI::Library
  ffi_lib ["m", "libm.so.6"]
  attach_function :pow, [:double, :double], :double
end
puts ListForm.pow(2.0, 10.0)

# A candidate zeo cannot decide is SKIPPED, not refused -- libusb leads with two
# locals holding bundled paths and only then names the system library.
module SkipsTheUndecidable
  extend FFI::Library
  bundled = "/nonexistent/libm.dylib"
  ffi_lib [bundled, "m"]
  attach_function :ceil, [:double], :double
end
puts SkipsTheUndecidable.ceil(1.2)

# A symbol names a library too -- windows gems write `ffi_lib :kernel32`.
module SymbolForm
  extend FFI::Library
  ffi_lib :m
  attach_function :sqrt, [:double], :double
end
puts SymbolForm.sqrt(144.0)

# `FFI::Platform::LIBC` is `FFI::Library::LIBC` by its other path, and either
# may be written `::`-anchored.
module LibcOtherPath
  extend FFI::Library
  ffi_lib ::FFI::Platform::LIBC
  attach_function :abs, [:int], :int
end
puts LibcOtherPath.abs(-11)
