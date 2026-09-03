# A `def` or `define_method` inside a `class_eval`/`module_eval` block installs
# on the receiver when the BLOCK runs, not when the enclosing class body was
# compiled -- so it has to displace whatever the class already answers.
#
# The definition itself always reached the right class: a brand-new name works
# even without any of this. What failed was the CALL -- a receiver whose class
# zeo knows folds to the compiled body and never asks the overlay. So the names
# a block-position `def` installs join `Compiler::runtime_redefs` beside the
# explicit `define_method` calls, and the fold stands down for them.
#
# Both call kinds needed it: an instance call and a `def self.` class-method
# call fold at different sites.

class N
  def d = "d0"
  def e = "e0"
end
n = N.new

N.class_eval { define_method(:d) { "d1" } }
p n.d

N.class_eval { def e = "e1" }
p n.e

# `module_eval` is the same method under its other name, and a class-method
# definition folds at a different site than an instance one.
module M
  def self.tag = "m0"
end
M.module_eval { def self.tag = "m1" }
p M.tag

# A brand-new name never had the problem -- there was no compiled body to fold
# to -- which is what showed the definee was right all along.
N.class_eval { def fresh = "f" }
p n.fresh

# Nested a block deeper, and reached through a local rather than the constant.
target = N
[1].each { target.class_eval { def deep = "deep" } }
p n.deep

# A class body's own `def` still folds: nothing here installs at runtime.
class Untouched
  def stable = "s"
end
p Untouched.new.stable
__END__
"d1"
"e1"
"m1"
"f"
"deep"
"s"
