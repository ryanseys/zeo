# Never loaded on this platform. The `:dword` is a windows FFI typedef this
# file declares nowhere, so eagerly compiling this file is a compile error --
# which is what makes the main file's assertions more than a `defined?` check.
require "ffi"

module WindowsUser
  extend FFI::Library

  class ProfileInfo < FFI::Struct
    layout :dw_size, :dword,
           :dw_flags, :dword
  end

  def self.admin? = true
end

WINDOWS_MARKER = :loaded
