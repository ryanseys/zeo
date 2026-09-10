# `class << self; def X; ...; end; end` inside a class body defines a
# singleton method on the enclosing class, the same as `def self.X`.

class C
  class << self
    def cls_method; "singleton"; end
    def with_arg(n); "got " + n.to_s; end
  end
end

puts C.cls_method
puts C.with_arg(42)

# Mixed form: regular `def self.X` plus a `class << self` block
# coexist in the same class.
class D
  def self.alpha; "a"; end
  class << self
    def beta; "b"; end
  end
end

puts D.alpha
puts D.beta
__END__
singleton
got 42
a
b
