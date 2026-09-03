# Never loaded on this platform -- and it must not be COMPILED either, which is
# what the `:dword` below proves: it is a windows FFI typedef zeo does not know,
# so following this require at all is a compile error.
require "ffi"

module WindowsOnly
  extend FFI::Library

  class ProfileInfo < FFI::Struct
    layout :dw_size, :dword,
           :dw_flags, :dword
  end
end

WINDOWS_MARKER = :loaded
