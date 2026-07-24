r = []
"a(b)c(d)".scan(/[()]/) { |t| r << t }
p r
p "a(b)".scan(/[()]/)
p "x1y2".scan(/(\d)/)
p "a[b]c".scan(/[\[\]]/)
