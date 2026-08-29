# An FFI integer slot converts the way the gem does, and THE SLOT'S WIDTH IS
# NOT THE RANGE CHECKED: the gem reaches C through NUM2INT/NUM2UINT for a 1-,
# 2- or 4-byte slot and NUM2LL/NUM2ULL for an 8-byte one. So a 1- or 2-byte
# slot truncates silently while a 4-byte one refuses past the 32-bit limits,
# and the four converters each name themselves in their own message.
#
# The unsigned converters wrap a negative number, which is why
# `write_uint64(-1)` reads back as 2**64-1 -- a value no `i64` holds, so the
# read has to answer a Bignum.

require "ffi"

m = FFI::MemoryPointer.new(:uint64, 2)

def t(label)
  r = begin
    yield.inspect
  rescue Exception => e
    "#{e.class}: #{e.message}"
  end
  puts format("%-18s %s", label, r)
end

t("u64 max") { m.write_uint64(0xFFFF_FFFF_FFFF_FFFF); m.read_uint64 }
t("u64 -1") { m.write_uint64(-1); m.read_uint64 }
t("u64 2**63") { m.write_uint64(0x8000_0000_0000_0000); m.read_uint64 }
t("u64 -2**63") { m.write_uint64(-2**63); m.read_uint64 }
t("u64 2**64") { m.write_uint64(2**64) }
t("u64 -2**63-1") { m.write_uint64(-2**63 - 1) }
t("i64 -1") { m.write_int64(-1); m.read_int64 }
t("i64 min") { m.write_int64(-2**63); m.read_int64 }
t("i64 2**63") { m.write_int64(2**63) }
t("i64 min-1") { m.write_int64(-2**63 - 1) }

t("u32 max") { m.write_uint32(0xFFFF_FFFF); m.read_uint32 }
t("u32 -1") { m.write_uint32(-1); m.read_uint32 }
t("u32 -2**31") { m.write_uint32(-2**31); m.read_uint32 }
t("u32 2**32") { m.write_uint32(2**32) }
t("u32 -2**31-1") { m.write_uint32(-2**31 - 1) }
t("i32 2**31") { m.write_int32(2**31) }
t("i32 -2**31-1") { m.write_int32(-2**31 - 1) }

# 8- and 16-bit slots truncate rather than refuse, but still go through the
# 32-bit converter, so they refuse the same values a 32-bit slot does.
t("u8 255") { m.write_uint8(255); m.read_uint8 }
t("u8 -1") { m.write_uint8(-1); m.read_uint8 }
t("u8 256") { m.write_uint8(256); m.read_uint8 }
t("i8 128") { m.write_int8(128); m.read_int8 }
t("u16 65536") { m.write_uint16(65536); m.read_uint16 }
t("u8 2**32") { m.write_uint8(2**32) }
t("u8 -2**31-1") { m.write_uint8(-2**31 - 1) }

# A Float truncates toward zero; out of range it names ITSELF, not "bignum".
t("u64 1.5") { m.write_uint64(1.5); m.read_uint64 }
t("u64 -1.5") { m.write_uint64(-1.5); m.read_uint64 }
t("u32 2.9") { m.write_uint32(2.9); m.read_uint32 }
t("u64 1e30") { m.write_uint64(1e30) }
t("i64 nan") { m.write_int64(Float::NAN) }

class Seven
  def to_int = 7
end
t("to_int") { m.write_uint64(Seven.new); m.read_uint64 }
t("string") { m.write_uint64("1") }
t("nil") { m.write_uint64(nil) }

t("array u64") { m.write_array_of_uint64([0xFFFF_FFFF_FFFF_FFFF, 1]); m.read_array_of_uint64(2) }
t("array i64 big") { m.write_array_of_int64([0xFFFF_FFFF_FFFF_FFFF]) }

t("inspect") { m.inspect }
t("inspect raw") { FFI::Pointer.new(4096).inspect }
t("inspect null") { FFI::Pointer::NULL.inspect }
t("inspect slice") { m.slice(0, 4).inspect }
t("to_s is inspect") { m.to_s == m.inspect }
