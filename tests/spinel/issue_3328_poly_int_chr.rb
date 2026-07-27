def hexd(n)
  n < 10 ? ("0".getbyte(0) + n).chr : ("A".getbyte(0) + n - 10).chr
end

def pct(byte)
  "%" + hexd(byte >> 4) + hexd(byte & 15)
end

p pct("=".getbyte(0))
p pct("&".getbyte(0))
x = [61, "a"]
v = x[0]
p v.chr
s = x[1]
p s.chr
