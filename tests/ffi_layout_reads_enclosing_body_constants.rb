# A layout can spell its types and counts as CONSTANTS of the ENCLOSING
# module -- ffi-ncurses declares its whole vocabulary that way above the
# structs that use it. Both halves of an inline array fold too.
require "ffi"

module WinStruct
  NCURSES_ATTR_T = :int
  WCHAR_T        = :ushort
  CCHARW_MAX     = 5

  class CCharT < FFI::Struct
    layout :attr, NCURSES_ATTR_T,
           :chars, [WCHAR_T, CCHARW_MAX]
  end
end

puts WinStruct::CCharT.offset_of(:chars)
puts WinStruct::CCharT.size
c = WinStruct::CCharT.new
c[:attr] = 7
c[:chars][0] = 65
c[:chars][4] = 90
puts c[:attr]
puts c[:chars][0]
puts c[:chars][4]
