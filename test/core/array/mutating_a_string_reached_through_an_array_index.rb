# `word[0] = "z"` on `path.last`, and a per-index copy built inside a block.
# (spinel issue #3172)
def f(word)
  word[0] = "z"
  word
end
def poison(start)
  path = [start]
  f(path.last)
end
p f("hot".dup)

def g(word)
  out = []
  word.chars.each_with_index { |_, i| c = word.dup; c[i] = "z"; out << c }
  out
end
def poison2(start) = g([start].last)
p g("hot")
__END__
"zot"
["zot", "hzt", "hoz"]
