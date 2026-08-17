# `b0 = bytes[i]` where bytes holds Integers, in a method that also appends to a
# String: the mutator table is name-keyed, so `b0 << 4` (an integer shift) read
# as a string append and the element was bound to a string handle (#3971).
ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"

def encode(s)
  bytes = s.bytes
  n = bytes.length
  out = +""
  i = 0
  while i + 3 <= n
    b0 = bytes[i]
    b1 = bytes[i + 1]
    b2 = bytes[i + 2]
    out << ALPHABET[(b0 >> 2) & 0x3F].to_s
    out << ALPHABET[((b0 << 4) | (b1 >> 4)) & 0x3F].to_s
    out << ALPHABET[((b1 << 2) | (b2 >> 6)) & 0x3F].to_s
    out << ALPHABET[b2 & 0x3F].to_s
    i = i + 3
  end
  out
end

p encode("Man")
p encode("any carnal pleasure")

# the string alias through a container still works
rows = [+"abc"]
r = rows[0]
r.upcase!
p [rows, r.equal?(rows[0])]
