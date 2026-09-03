# `[]`-style compound assignment with MORE than one index. `[]`/`[]=` are
# ordinary methods, so `a[i, j] += rhs` is a two-argument `[]` paired with a
# three-argument `[]=` -- Array's `(start, length)` splice form. Each index
# binds to its own hidden local so a side-effecting index runs exactly once
# across the read and the write, the same guarantee the receiver already had.

a = [1, 2, 3, 4]
a[1, 2] += ["x"]
p a

b = [1, 2, 3]
b[0, 2] ||= 9
p b

$i = 0
$j = 0
def idx; $i += 1; 0; end
def len; $j += 1; 1; end
c = [1, 2, 3]
c[idx, len] = [9]
p c
p [$i, $j]
__END__
[1, 2, 3, "x", 4]
[1, 2, 3]
[9, 2, 3]
[1, 1]
