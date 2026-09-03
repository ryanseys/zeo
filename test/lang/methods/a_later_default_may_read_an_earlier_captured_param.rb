# `def fill_breakable(sep = ' ', width = sep.length)` with `sep`
# captured by a block (prettyprint's shape): `width`'s default reads
# `sep` through its capture cell, so the cell must exist BEFORE the
# later default evaluates -- wraps now interleave with the bindings in
# parameter order instead of trailing the whole prologue.

def fill(sep = "--", width = sep.length)
  t = proc { sep }
  [t.call, width]
end
p fill
p fill("abc")
p fill("abc", 9)
__END__
["--", 2]
["abc", 3]
["abc", 9]
