# rjust, ljust and center whose width comes from `keys.max`, with and without a
# pad string.
# (spinel issue #3169)
counts = Hash.new(0)
counts[1] = 1; counts[4] = 1
w = counts.keys.max
p "x".rjust(w)
p "y".ljust(w)
p "z".center(w)
p "ab".rjust(w, "*")
p "cd".ljust(w, "-")
# static int width still works
p "s".rjust(5)
p "t".ljust(5, ".")
__END__
"   x"
"y   "
" z  "
"**ab"
"cd--"
"    s"
"t...."
