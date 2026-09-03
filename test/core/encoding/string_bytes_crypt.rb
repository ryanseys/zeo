# String#append_as_bytes and String#crypt.

# append_as_bytes: integers contribute their low byte, strings their bytes.
s = "abc".dup
s.append_as_bytes("de")
p s
s = "x".dup
s.append_as_bytes(0x41, 0x42, "CD", 256)
p s.bytes
p s.encoding

# Negative and big integers wrap to a single low byte.
s = "x".dup
s.append_as_bytes(-1, 2**70)
p s.bytes

# Encoding is preserved (a binary receiver stays binary).
s = "hi".b
s.append_as_bytes(0xFF)
p s.encoding

# A non String/Integer argument is a TypeError.
begin
  "x".dup.append_as_bytes(:sym)
rescue TypeError => e
  puts e.message
end

# crypt: the host crypt(3) hash, ASCII-8BIT result.
p "password".crypt("ab")
p "password".crypt("ab").encoding

# crypt validates the salt and rejects a NUL in the key.
begin; "x".crypt("a"); rescue ArgumentError => e; puts e.message; end
begin; "x".crypt(""); rescue ArgumentError => e; puts e.message; end
begin; "x\0y".crypt("ab"); rescue ArgumentError => e; puts e.message; end
__END__
"abcde"
[120, 65, 66, 67, 68, 0]
#<Encoding:UTF-8>
[120, 255, 0]
#<Encoding:BINARY (ASCII-8BIT)>
wrong argument type Symbol (expected String or Integer)
"abJnggxhB/yWI"
#<Encoding:BINARY (ASCII-8BIT)>
salt too short (need >=2 bytes)
salt too short (need >=2 bytes)
string contains null byte
