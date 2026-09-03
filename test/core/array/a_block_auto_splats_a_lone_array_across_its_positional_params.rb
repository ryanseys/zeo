# CRuby's `has_lead && !ambiguous_param0` rule -- see
# `clif::params::auto_splats`.

def one(v)
  yield v
end
p(one([1, 2]) { |a, b| [a, b] })
p(one([1, 2]) { |a| a })
p(one([1, 2]) { |*a| a })
p(one([1, 2]) { |a, *b| [a, b] })
p(one([1, 2]) { |a, **k| [a, k] })
p [[1, 2], [3, 4]].map { |a, b| a + b }
p({ x: 1 }.map { |k, v| [k, v] })
__END__
[1, 2]
[1, 2]
[[1, 2]]
[1, [2]]
[[1, 2], {}]
[3, 7]
[[:x, 1]]
