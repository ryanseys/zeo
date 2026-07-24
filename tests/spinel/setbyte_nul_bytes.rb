# Regression: String#setbyte sized the receiver with strlen, which stops at
# the first NUL byte. An all-NUL buffer (the pack("C*") byte-buffer pattern)
# measured length 0, so every setbyte raised IndexError despite length/
# bytesize correctly reporting the stored length. setbyte must use the
# NUL-safe stored length like getbyte/bytesize do.

# all-NUL buffer: previously every call here raised IndexError
s = Array.new(5, 0).pack("C*")
puts "len=#{s.length}"
s.setbyte(0, 65)
s.setbyte(4, 90)
puts s.bytes.inspect

# NUL-prefixed buffer, negative index counts from the true end
t = [0, 0, 66, 0].pack("C*")
t.setbyte(-1, 67)
t.setbyte(0, 68)
puts t.bytes.inspect

# out-of-range past the stored length still raises
begin
  s.setbyte(5, 1)
  puts "no error"
rescue IndexError => e
  puts "IndexError: #{e.message}"
end

# setbyte return value is the byte written
puts s.setbyte(1, 200)
puts s.bytes.inspect
