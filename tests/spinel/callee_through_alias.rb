class A1
  def a; __callee__; end
  alias b a
  alias_method :c, :a
  def m; __method__; end
  alias n m
  def both; [__method__, __callee__]; end
  alias both2 both
end
o = A1.new
p o.a
p o.b
p o.c
p o.m
p o.n
p o.both
p o.both2

def top; __callee__; end
alias top2 top
p top
p top2

class Sub < A1; end
s = Sub.new
p s.b
p s.a

class Chain
  def orig; __callee__; end
  alias one orig
  alias two one
end
p Chain.new.two
p Chain.new.one
p Chain.new.orig
