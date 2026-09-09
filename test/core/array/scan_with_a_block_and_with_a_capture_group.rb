# The block sees each match; without one, a capture group makes each result an
# array.
# (spinel issue #2912)
r = []
"a(b)c(d)".scan(/[()]/) { |t| r << t }
p r
p "a(b)".scan(/[()]/)
p "x1y2".scan(/(\d)/)
p "a[b]c".scan(/[\[\]]/)
__END__
["(", ")", "(", ")"]
["(", ")"]
[["1"], ["2"]]
["[", "]"]
