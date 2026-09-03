# `pop`/`shift` answer ONE element; `pop(n)`/`shift(n)` answer an ARRAY
# -- a different return type, not just a different count, which is why
# the no-arg form can't be `pop(1)`. Both were `arity!(args, 0)`, so the
# count form raised a spurious ArgumentError on a call Ruby accepts.

a = [1, 2, 3, 4]
p a.pop(2)
p a
b = [1, 2, 3, 4]
p b.shift(2)
p b
p [1, 2].pop(5)
p [1, 2].pop(0)
c = [1, 2]
p c.shift(0)
p c
p [1, 2].pop
p [].pop
p [].shift
__END__
[3, 4]
[1, 2]
[1, 2]
[3, 4]
[1, 2]
[]
[]
[1, 2]
2
nil
nil
