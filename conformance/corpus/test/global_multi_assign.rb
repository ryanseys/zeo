# Multi-assignment into global variables must declare and type every
# target from the right-hand side, not only the first. A non-int RHS
# (string / float / array) or a splat-rest target previously left the C
# global declared as mrb_int and failed to compile.
$g1, $g2 = 1, 2
p [$g1, $g2]

$s1, $s2 = "x", "y"
p [$s1, $s2]

$f1, $f2 = 1.5, 2.5
p [$f1, $f2]

$pp, $qq = [10, 20]
p [$pp, $qq]

# mixed local and global targets
first, $g = 1, 2
p [first, $g]

# splat-rest target collects into an array global
$x, *$rest = 1, 2, 3, 4
p $x
p $rest

# heterogeneous targets
$h1, $h2, $h3 = "a", 2, 3.0
p [$h1, $h2, $h3]
