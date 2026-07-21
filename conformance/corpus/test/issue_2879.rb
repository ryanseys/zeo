TBL = {}
"abc".each_char.with_index { |ch, i| TBL[ch] = i }

def parse(s, base)
  s.each_char.reduce(0) { |acc, ch| acc * base + TBL[ch] }
end

p parse("bca", 10)

# a constant empty hash with direct writes
T2 = {}
T2["a"] = 1
p T2["a"]

# a constant empty hash never written still has a slot
T3 = {}
p T3.size
p T3.empty?

# symbol-keyed constant
T4 = {}
T4[:k] = "v"
p T4[:k]
p T4
