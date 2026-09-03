# `X.singleton_class.prepend(M)` where X only ever exists at RUNTIME (a
# `Class.new` minting -- the same shape as a rails class behind a computed
# autoload): M's instance methods become X's class methods ABOVE X's own
# `def self.x`, and `super` from one resumes at the shadowed definition.
# The compile-time ancestry edit cannot name X, so this exercises the runtime
# layer (`prepend_into_class_singleton` + the prepend-aware `super` resume);
# a statically-registered receiver still takes the compile-time edit instead.
# The subclass call checks the prepend is visible through inheritance -- the
# ancestor walk finds the prepend layer at the parent's position.
K = Class.new do
  def self.hi = "orig"
end
module Patch
  def hi = "patched-" + super
  def extra = "extra"
end
K.singleton_class.prepend(Patch)
p K.hi
p K.extra
p K.respond_to?(:extra)
Sub = Class.new(K)
p Sub.hi
__END__
"patched-orig"
"extra"
true
"patched-orig"
