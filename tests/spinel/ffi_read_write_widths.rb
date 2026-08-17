# Every typed accessor names the WIDTH of its access. The bytes below are laid
# out by a wider write and read back one width at a time, so a reader that
# loaded the wrong width would fold in its neighbours. Little-endian target
# (x86_64 / arm64), which is what the FFI layer assumes.
#
# Ported from spinel's ffi_read_write_widths, whose `ffi_buffer` /
# `ffi_read_u32` / `ffi_write_i16` class macros are compile-time spinel DSL
# with no CRuby analog. The real ffi gem spells the same accesses as
# FFI::MemoryPointer#put_*/#get_* at an offset, which is what this checks.
require "ffi"

buf = FFI::MemoryPointer.new(:uint8, 32)

buf.put_uint32(0, 0x04030201)
puts buf.get_uint8(0)
puts buf.get_uint8(1)
puts buf.get_uint8(2)
puts buf.get_uint8(3)
puts buf.get_uint16(0)
puts buf.get_uint16(2)
puts buf.get_uint32(0)

buf.put_int8(8, -5)
puts buf.get_int8(8)
puts buf.get_uint8(8)

buf.put_int16(10, -300)
puts buf.get_int16(10)
puts buf.get_uint16(10)

buf.put_int64(16, -1234567890123)
puts buf.get_int64(16)

# a 2-byte store leaves the two bytes above it alone
buf.put_uint16(24, 0xBEEF)
puts buf.get_uint32(24)

# the last byte of the buffer: a wider load here would run off the end
puts buf.get_uint8(31)
