# Class-level @ivars are NOT inherited: each class holds its own, and an
# inherited class method binds to the CALLING class's storage.
class Base
  @tag = "base"
  def self.tag = @tag
  def self.set(v); @tag = v; end
end
class Sub < Base
end
Base.set("B")
Sub.set("S")
p Base.tag      # "B" -- Base's own @tag
p Sub.tag       # "S" -- Sub's own @tag (not Base's)

# read-only: a subclass that never sets its @ivar reads nil, not the parent's
class Reader
  @v = "r"
  def self.v = @v
end
class RSub < Reader
end
p Reader.v      # "r"
p RSub.v        # nil

# three levels + counter mutation, each class independent
class A
  @c = 0
  def self.bump; @c += 1; @c; end
  def self.c = @c
end
class B < A
  @c = 100
end
class C < B
end
A.bump; A.bump
B.bump
p A.c           # 2
p B.c           # 101
p C.c           # nil
