# `<=>` between two self-referential arrays answers 0 in ruby (the paired
# recursion guard treats a revisited PAIR as equal); zeo recurses to a stack
# overflow and the process ABORTS. `==` already has the guard; `<=>` (and so
# `Comparable` over such arrays) lacks it. (Found by the 2026-08-24 probe
# sweep.)
a = [1]
a << a
b = [1]
b << b
p a == b
p(a <=> b)
