# Three declaration forms the real `ffi` gem takes and zeo refused.
require "ffi"

# 1. C typedefs. The gem resolves ~200 of them from the headers of the machine
#    it runs on; zeo takes only the ones whose width and signedness are the
#    same on every target it builds for -- the C99 exact-width names, the
#    pointer-width names, and the POSIX types that agree. `mode_t`, `dev_t`,
#    `nlink_t`, `sa_family_t` and friends genuinely differ between macOS and
#    glibc, and stay a clean rejection instead of a silently wrong layout.
module Typed
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :abs, [:int32_t], :int32_t
  attach_function :my_strlen, :strlen, [:string], :size_t
  attach_function :getpid, [], :pid_t
  attach_function :getuid, [], :uid_t
end
p Typed.abs(-5)
p Typed.my_strlen("hello")
p Typed.getpid == Process.pid
p Typed.getuid == Process.uid

# The same names work as struct fields, where the width also fixes the offsets.
class Sized < FFI::Struct
  layout :a, :uint8_t, :b, :int32_t, :c, :uintptr_t
end
p [Sized.size, Sized.offset_of(:b), Sized.offset_of(:c)]

# 2. `attach_function`'s options hash. `blocking: true` releases the GVL for
#    the call, which is a no-op under zeo's default parallel threads and the
#    real handoff under ZEO_GVL=1.
module Blocking
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :labs, [:int64_t], :int64_t, blocking: true
end
p Blocking.labs(-9)

# 3. An enum member's value written as the expression it is. Flag enums are
#    spelled as shifts and ors far more often than as the numbers they come to.
#    The folded values are what the marshaling tables are built from, so they
#    are checked where they are observable: a member symbol passed in as the
#    enum type, and the same number read back out.
module Flags
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  enum :mode, [:none, 0, :lazy, (1 << 0), :eager, (1 << 1), :both, (1 << 0) | (1 << 1)]
  attach_function :as_int, :abs, [:mode], :int
  attach_function :as_mode, :abs, [:int], :mode
end
p [Flags.as_int(:none), Flags.as_int(:lazy), Flags.as_int(:eager), Flags.as_int(:both)]
p [Flags.as_mode(0), Flags.as_mode(1), Flags.as_mode(2), Flags.as_mode(3)]

# An enum-typed field in a struct declared by the SAME library carries the
# folded values into the layout too.
module Flags
  class Holder < FFI::Struct
    layout :m, :mode, :n, :int32
  end
end
h = Flags::Holder.new
h[:m] = :eager
p [h[:m], Flags::Holder.size, Flags::Holder.offset_of(:n)]
