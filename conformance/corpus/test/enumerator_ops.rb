# Enumerator ops: chained each_with_index, +, next_values/peek_values, product
# (#2487, #2481, #2482, #2484)
e = [10, 20].each.each_with_index
e.each { |x, i| p [x, i] }            # 2487
a = [1, 2].each; b = [3, 4].each
p((a + b).to_a)                        # 2481
c = [1, 2].each
p(c.next_values)                       # 2482
p(c.peek_values)
p(Enumerator.product([1, 2], [3, 4]).to_a)          # 2484
p(Enumerator.product([1, 2], [3], [4, 5]).to_a)     # 2484 (3 args)
