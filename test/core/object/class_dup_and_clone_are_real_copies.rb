# `Class#dup`/`#clone` mint an ANONYMOUS class sharing the source's superclass
# and mixins and carrying copies of its own rows, constants and class-level
# state. Handing back the same handle made `K.dup.equal?(K)` true, so naming
# the copy renamed `K` and editing it edited the original.
class K
  X = 1
  @civ = :c
  @@cv = :cv
  def self.civ = @civ
  def self.cls = :cls
  def m = :m
  private def pm = :pm
end

d = K.dup
c = K.clone
p [d.name, c.name, K.name]
p [d.equal?(K), c.equal?(K), d.equal?(c)]
p [d.class, c.class]
p d.superclass.to_s
p [d.singleton_methods.sort, c.singleton_methods.sort]
p [d.instance_variable_get(:@civ), c.civ]
p [d.constants, d.const_get(:X)]
p d.instance_methods(false).sort
p d.private_instance_methods(false)
p d.class_variable_get(:@@cv)
p d.new.m
p d.new.is_a?(K)
p d.new.class.equal?(d)

# Editing the copy leaves the source alone.
d.const_set(:Y, 2)
p [d.constants.sort, K.constants.sort]
NamedCopy = K.dup
p [NamedCopy.name, K.name]

# Mixins ride along.
module Mix
  def mx = :mx
end
class L
  include Mix
end
p L.dup.ancestors.map(&:to_s).drop(1)
p L.dup.new.mx

# A subclass copy keeps its real superclass.
class Sub < K; end
p Sub.dup.superclass.to_s
p Sub.dup.new.m

# `clone` carries the frozen state; `dup` never does.
K.freeze
p [K.dup.frozen?, K.clone.frozen?, K.clone(freeze: false).frozen?]

# A MODULE copy still works the way it always did.
p [Mix.dup.name, Mix.dup.equal?(Mix)]
p Mix.dup.instance_methods(false)
__END__
[nil, nil, "K"]
[false, false, false]
[Class, Class]
"Object"
[[:civ, :cls], [:civ, :cls]]
[:c, :c]
[[:X], 1]
[:m]
[:pm]
:cv
:m
false
true
[[:X, :Y], [:X]]
["NamedCopy", "K"]
["Mix", "Object", "Kernel", "BasicObject"]
:mx
"K"
:m
[false, true, false]
[nil, false]
[:mx]
