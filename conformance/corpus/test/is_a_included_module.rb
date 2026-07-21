module M
  def foo; 42; end
end

module N
end

class C
  include M
end

class D
  include M
  include N
end

c = C.new
p c.foo
p c.is_a?(M)
p c.is_a?(N)
p C.ancestors.include?(M)
p C.ancestors.include?(N)

d = D.new
p d.is_a?(M)
p d.is_a?(N)
p D.ancestors.include?(M)
p D.ancestors.include?(N)
