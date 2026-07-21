# Enumerator#with_object with no block -> an Enumerator (#2540)
e = [1, 2, 3].each.with_object([])
p(e.class)
p(e.to_a)                                   # [[elem, memo], ...]
r = [1, 2, 3].each.with_object([]) { |x, m| m << x * 2 }
p(r)                                        # block form still returns the memo
