# `alias $copy $orig` is a real alias, not a copy: one slot, two names,
# and writing EITHER is visible through the other. Oracle-verified both
# ways -- which is why it can't lower to `$copy = $orig`.

$orig = 5
alias $copy $orig
$copy = 7
p $orig
p $copy
$orig = 9
p [$orig, $copy]
alias $b $never_set
p $b
$never_set = 1
p $b
__END__
7
7
[9, 9]
nil
1
