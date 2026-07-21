# Adapted from yjit-bench ruby-xor
# XOR two strings byte-by-byte

def xor_strings(a, b)
  len = a.bytesize
  result = a.dup
  i = 0
  while i < len
    result.setbyte(i, a.getbyte(i) ^ b.getbyte(i))
    i = i.succ
  end
  result
end

a = "abcdefghijklmnop" * 100
b = "qrstuvwxyz012345" * 100

n = 10000
i = 0
while i < n
  xor_strings(a, b)
  i += 1
end

result = xor_strings(a, b)
puts result.bytesize
puts result.getbyte(0)
puts result.getbyte(1)
