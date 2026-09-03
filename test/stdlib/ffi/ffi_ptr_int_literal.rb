# Sentinel integer-valued pointers, ported to the real ffi gem API. The gem
# spells an integer-valued pointer as FFI::Pointer.new(n) -- e.g. -1 for
# sqlite3_bind_text's SQLITE_TRANSIENT destructor sentinel. free(NULL) is a
# well-defined POSIX no-op, so a NULL-valued sentinel (and nil, the gem's
# other NULL spelling) is the safe observation point.
require "ffi"

module LibC
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :free, [:pointer], :void
end

# Integer-valued NULL pointer as a :pointer arg.
LibC.free(FFI::Pointer.new(0))

# nil is the gem's NULL for a :pointer arg.
LibC.free(nil)

# A non-NULL sentinel constructs (not passed to free -- it isn't a real
# allocation); its bit pattern is preserved.
sentinel = FFI::Pointer.new(-1)
puts(sentinel.null? ? "sentinel_null" : "sentinel_non_null")

puts "ok"
__END__
sentinel_non_null
ok
