# An `ffi_lib` whose name only a RUNNING process can resolve -- a path built
# by interpolation -- takes the dlopen/dlsym tier: the first candidate that
# opens wins, exactly as the gem tries its alternatives. The list spans both
# platforms' libm spellings; the leading candidate never exists.

require "ffi"
module M
  extend FFI::Library
  ffi_lib ["#{'/no'}/such/dir/libnothing.so", "/usr/lib/libm.dylib", "libm.so.6"]
  attach_function :pow, [:double, :double], :double
end
puts M.pow(2.0, 8.0)
__END__
256.0
