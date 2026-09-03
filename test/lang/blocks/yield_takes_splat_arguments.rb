# `HirNode::Yield` carries the same `Vec<ArrayElem>` a Call's positional
# args do, so a splat flattens at runtime and the block then binds from
# the result through its ordinary parameter machinery -- auto-splat,
# rest, and nil-padding of surplus params all included.

def m(*a); yield(*a); end
p(m(1, 2) { |x, y| [x, y] })
p(m(1) { |x, y| [x, y] })
p(m() { |x, y| [x, y] })

def mix(*a); yield(0, *a, 9); end
p(mix(1, 2) { |*z| z })

def empty; yield(*[]); end
p(empty { |*z| z })

def kwmix(*a); yield(*a, k: 1); end
p(kwmix(1) { |x, k:| [x, k] })

class W
  def initialize(n); @n = n; end
  attr_reader :n
end
def objs; yield(*[W.new(1), W.new(2)]); end
p(objs { |a, b| a.n + b.n })
__END__
[1, 2]
[1, nil]
[nil, nil]
[0, 1, 2, 9]
[]
[1, 1]
3
