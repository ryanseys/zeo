# A single SPLAT is the case a plain "one argument -> pass it through"
# rule gets wrong: `return *a` with `a == [1]` is `[1]`, not `1`.

def two; return 1, 2; end
p two
def three; return 1, 2, 3; end
p three
def splat_ret(a); return *a; end
p splat_ret([1, 2])
p splat_ret([1])
def mixed(a); return 1, *a; end
p mixed([2, 3])
p([1].each { break 1, 2 })
p([[1, 2]].map { |a, b| next a, b })
def none; return; end
p none
__END__
[1, 2]
[1, 2, 3]
[1, 2]
[1]
[1, 2, 3]
[1, 2]
[[1, 2]]
nil
