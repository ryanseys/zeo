class A
  def m(a = 1, *rest, k: 2, **kw) = [a, rest, k, kw]
end

class B < A
  def m(*) = super
end

p B.new.m(9, k: 3, z: 4)
__END__
[9, [{k: 3, z: 4}], 2, {}]
