# awk-mode split honors limit (1 = whole, cap = verbatim remainder, nonzero
# keeps a trailing empty); $1 is live inside a gsub/sub block.

s = " one two  three "
sep = " "
p s.split(sep, 1)
p s.split(sep, 2)
p s.split(sep, -1)
p "   ".split(sep, -1)
p "trail   ".split(sep, 3)
p "a1b2c3".gsub(/(\d)/) { ($1.to_i * 2).to_s }
p "x9".sub(/(\d)/) { ($1.to_i + 1).to_s }
__END__
[" one two  three "]
["one", "two  three "]
["one", "two", "three", ""]
[""]
["trail", ""]
"a2b4c6"
"x10"
