# `each_byte` / `each_codepoint` with a block on a receiver only known to be a
# String at run time. The poly path served each_char but not these, so the call
# fell through to NoMethodError.

def pick(f, s)
  f ? s : 42
end

v = pick(true, "hello")
n = 0
v.each_byte { |b| n += b }
p n
c = 0
v.each_codepoint { |cp| c += cp }
p c
