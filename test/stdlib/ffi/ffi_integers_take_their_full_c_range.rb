# Every FFI integer type accepts its whole C range in both directions: a
# :ulong/:uint64/:size_t argument or return reaches 2**64-1 (a Bignum), a
# :long/:int64 reaches -2**63, and a value outside the C type raises the
# gem's own RangeError -- which names the converter (:long is not :int64).
require "ffi"

module L
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :strtoull, [:string, :pointer, :int], :ulong
  attach_function :strtoll, [:string, :pointer, :int], :long
  attach_function :strnlen_ulong, :strnlen, [:string, :ulong], :ulong
  attach_function :strnlen_u64, :strnlen, [:string, :uint64], :uint64
  attach_function :strnlen_size_t, :strnlen, [:string, :size_t], :size_t
  attach_function :strnlen_long, :strnlen, [:string, :long], :long
  attach_function :strnlen_i64, :strnlen, [:string, :int64], :int64
  attach_function :strnlen_u32, :strnlen, [:string, :uint32], :uint32
  attach_function :strnlen_i8, :strnlen, [:string, :int8], :int8
  attach_function :llabs, [:long_long], :long_long
  attach_function :labs, [:long], :long
  attach_function :abs, [:int], :int
  attach_function :snprintf, [:pointer, :size_t, :string, :varargs], :int
end

# Returns above i64::MAX and at i64::MIN.
p L.strtoull("18446744073709551615", nil, 10)
p L.strtoll("-9223372036854775808", nil, 10)

# Arguments at both ends of every 64-bit spelling; a negative wraps for an
# unsigned type, exactly as NUM2ULONG does.
p L.strnlen_ulong("abc", 2**64 - 1)
p L.strnlen_u64("abc", 2**64 - 1)
p L.strnlen_size_t("abc", 2**64 - 1)
p L.strnlen_ulong("abc", -1)
p L.strnlen_long("abc", 2**63 - 1)
p L.strnlen_i64("abc", 2**63 - 1)
p L.llabs(-2**63 + 1)
p L.labs(-2**63 + 1)
p L.strnlen_u32("abc", 2**32 - 1)
p L.strnlen_u32("abc", -1)
p L.strnlen_i8("abc", 2.9)

# Variadic arguments promote but keep their own converter.
buf = FFI::MemoryPointer.new(:char, 128)
L.snprintf(buf, 128, "%llu %lld %u %d", :ulong, 2**64 - 1, :long, -2**63, :uint32, 2**32 - 1, :int32, -2**31)
puts buf.read_string
L.snprintf(buf, 128, "%llu %u %d", :uint64, 2**64 - 1, :uint8, -1, :int8, 200)
puts buf.read_string

# A pointer round-trips the full unsigned range.
mp = FFI::MemoryPointer.new(:uint64, 3)
mp.put_uint64(0, 2**64 - 1)
p mp.read_uint64
mp.write_ulong(2**64 - 1)
p mp.read_ulong
mp.put_array_of_uint64(0, [2**64 - 1, 1, 2**63])
p mp.read_array_of_ulong(3)

# A callback receives and returns the full range.
f = FFI::Function.new(:ulong, [:ulong]) { |x| p [:cb, x]; x }
p f.call(2**64 - 1)
g = FFI::Function.new(:long, [:long]) { |x| x }
p g.call(-2**63)
h = FFI::Function.new(:int, [:int8]) { |x| x }
p h.call(300)
p FFI::Function.new(:int, [:int]) { nil }.call(1)

# A struct field goes through the same converters.
class S < FFI::Struct
  layout :a, :ulong, :b, :long, :c, :uint64
end
s = S.new
s[:a] = 2**64 - 1
s[:b] = -2**63
s[:c] = 2**64 - 1
p [s[:a], s[:b], s[:c]]
p S.layout.fields.map { |fld| fld.type }

def attempt
  yield
  puts "no error"
rescue RangeError, TypeError => e
  puts "#{e.class}: #{e.message}"
end

# Out of range, and the wrong kind of value: the gem's exact wording.
attempt { L.strnlen_ulong("abc", 2**64) }
attempt { L.strnlen_ulong("abc", -2**63 - 1) }
attempt { L.strnlen_ulong("abc", -2**64) }
attempt { L.strnlen_size_t("abc", 2**64) }
attempt { L.strnlen_u64("abc", 2**64) }
attempt { L.strnlen_u64("abc", -2**63 - 1) }
attempt { L.strnlen_long("abc", 2**63) }
attempt { L.strnlen_long("abc", -2**63 - 1) }
attempt { L.strnlen_i64("abc", 2**63) }
attempt { L.abs(2**31) }
attempt { L.abs(-2**31 - 1) }
attempt { L.abs(2**64) }
attempt { L.strnlen_u32("abc", 2**32) }
attempt { L.strnlen_u32("abc", -2**31 - 1) }
attempt { L.strnlen_u32("abc", 2**63) }
attempt { L.strnlen_i8("abc", 2**31) }
attempt { L.strnlen_ulong("abc", 2.0**64) }
attempt { L.strnlen_u64("abc", 2.0**64) }
attempt { L.strnlen_long("abc", 2.0**63) }
attempt { L.strnlen_i64("abc", Float::NAN) }
attempt { L.strnlen_ulong("abc", nil) }
attempt { L.strnlen_long("abc", nil) }
attempt { L.strnlen_i64("abc", nil) }
attempt { L.strnlen_i64("abc", true) }
attempt { L.strnlen_i64("abc", "1") }
attempt { L.abs("1") }
attempt { L.snprintf(buf, 128, "%llu", :ulong, 2**64) }
attempt { L.snprintf(buf, 128, "%u", :uint8, 2**32) }
attempt { mp.put_ulong(0, 2**64) }
attempt { mp.put_uint64(0, 2**64) }
attempt { mp.write_long(2**63) }
attempt { mp.write_int64(2**63) }
attempt { mp.write_array_of_ulong([2**64]) }
attempt { mp.write_uint(2**32) }
attempt { s[:a] = 2**64 }
attempt { s[:b] = 2**63 }
attempt { s[:c] = -2**63 - 1 }
attempt { f.call(2**64) }
attempt { FFI::Function.new(:ulong, [:ulong]) { 2**64 }.call(1) }
attempt { FFI::Function.new(:uint8, [:int]) { 2**32 }.call(1) }
__END__
18446744073709551615
-9223372036854775808
3
3
3
3
3
3
9223372036854775807
9223372036854775807
3
3
2
18446744073709551615 -9223372036854775808 4294967295 -2147483648
18446744073709551615 4294967295 200
18446744073709551615
18446744073709551615
[18446744073709551615, 1, 9223372036854775808]
[:cb, 18446744073709551615]
18446744073709551615
-9223372036854775808
44
0
[18446744073709551615, -9223372036854775808, 18446744073709551615]
[#<FFI::Type::Builtin::ULONG size=8 alignment=8>, #<FFI::Type::Builtin::LONG size=8 alignment=8>, #<FFI::Type::Builtin::UINT64 size=8 alignment=8>]
RangeError: bignum too big to convert into 'unsigned long'
RangeError: bignum out of range of unsigned long
RangeError: bignum too big to convert into 'unsigned long'
RangeError: bignum too big to convert into 'unsigned long'
RangeError: bignum too big to convert into 'unsigned long long'
RangeError: bignum out of range of unsigned long long
RangeError: bignum too big to convert into 'long'
RangeError: bignum too big to convert into 'long'
RangeError: bignum too big to convert into 'long long'
RangeError: integer 2147483648 too big to convert to 'int'
RangeError: integer -2147483649 too small to convert to 'int'
RangeError: bignum too big to convert into 'long'
RangeError: integer 4294967296 too big to convert to 'unsigned int'
RangeError: integer -2147483649 too small to convert to 'unsigned int'
RangeError: integer 9223372036854775808 too big to convert to 'unsigned int'
RangeError: integer 2147483648 too big to convert to 'int'
RangeError: float 1.844674407e+19 out of range of integer
RangeError: float 1.844674407e+19 out of range of unsigned long long
RangeError: float 9.223372037e+18 out of range of integer
RangeError: float NaN out of range of long long
TypeError: no implicit conversion of nil into Integer
TypeError: no implicit conversion from nil to integer
TypeError: no implicit conversion from nil
TypeError: no implicit conversion from boolean
TypeError: no implicit conversion from string
TypeError: no implicit conversion of String into Integer
RangeError: bignum too big to convert into 'unsigned long'
RangeError: integer 4294967296 too big to convert to 'unsigned int'
RangeError: bignum too big to convert into 'unsigned long'
RangeError: bignum too big to convert into 'unsigned long long'
RangeError: bignum too big to convert into 'long'
RangeError: bignum too big to convert into 'long long'
RangeError: bignum too big to convert into 'unsigned long'
RangeError: integer 4294967296 too big to convert to 'unsigned int'
RangeError: bignum too big to convert into 'unsigned long'
RangeError: bignum too big to convert into 'long'
RangeError: bignum out of range of unsigned long long
RangeError: bignum too big to convert into 'unsigned long'
RangeError: bignum too big to convert into 'unsigned long'
RangeError: integer 4294967296 too big to convert to 'unsigned int'
