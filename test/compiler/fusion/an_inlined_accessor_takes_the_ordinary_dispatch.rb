# The widened sweep behind `an_inlined_accessor_ignores_a_later_redefinition`:
# every route by which a run-time definition can displace an accessor the
# compiler folded to an ivar read. Each pair below reads the same accessor
# from INSIDE the class and from outside, so a fold that outlived its
# redefinition shows as two answers for one method.
#
# The routes: a reader and a writer replaced by `define_method`; the same
# name replaced on a SUBCLASS, where the superclass keeps its own answer; a
# `remove_method` followed by an `include` that supplies the name from a
# module; and an `alias_method` over the accessor.

class A
  attr_accessor :v
  def initialize = @v = 1
  def read = v
  def write(x) = (self.v = x)
end
a = A.new
p a.read
A.class_eval { define_method(:v) { 99 } }
p a.read
A.class_eval { define_method(:v=) { |x| @v = x * 10 } }
p a.write(5)
p a.instance_variable_get(:@v)

class B
  attr_reader :w
  def initialize = @w = 2
  def read = w
end
class C < B
  def read2 = w
end
c = C.new
p c.read
p c.read2
C.class_eval { define_method(:w) { :sub } }
p c.read
p c.read2
p B.new.read

module M
  def z = :module_z
end
class D
  attr_reader :z
  def initialize = @z = 3
  def read = z
end
d = D.new
p d.read
D.class_eval { remove_method :z }
D.include(M)
p d.read

class E
  attr_reader :q
  def initialize = @q = 4
  def read = q
end
e = E.new
p e.read
E.class_eval { alias_method :q, :object_id }
p e.read.is_a?(Integer)
__END__
1
99
5
50
2
2
:sub
:sub
2
3
:module_z
4
true
