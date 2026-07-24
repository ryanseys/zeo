# A prepended module precedes the class in #ancestors (#2702). Dispatch already
# honoured prepend; only the ancestor listing was missing it.
module M; def hi; "M"; end; end
class C; prepend M; def hi; "C"; end; end
p C.ancestors
p C.ancestors.include?(M)
p C.new.hi

# last prepend lands closest to the front; includes still follow the class
module A; end
module B; end
module I; end
class Multi
  prepend A
  prepend B
  include I
end
p Multi.ancestors
p(Multi <= A)
p(Multi < B)

# a class with no prepend is unchanged
class Plain; include I; end
p Plain.ancestors

# prepend is visible through inheritance
module P2; end
class Base2; prepend P2; end
class Sub2 < Base2; end
p Sub2.ancestors
