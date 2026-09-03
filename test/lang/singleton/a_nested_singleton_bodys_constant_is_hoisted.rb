# zeo hoists a `class << self` body's constant one level out -- documented in
# docs/COMPATIBILITY.md, and unchanged since the surrogate was first minted.
# A NESTED `class << self` body's constant hoists the same one level: ruby
# homes it on `K.singleton_class.singleton_class`, zeo on
# `K.singleton_class`.
#
# The property that carries the corpus is unaffected: the `def` beside it
# still reads it by bare name, because both live on the same class either way.
class K
  class << self
    class << self
      SUFFIX = "!"
      def shout(s) = s + SUFFIX
    end
  end
end
p K.singleton_class.shout("hi")
p K.singleton_class.singleton_class.const_defined?(:SUFFIX)
p K.singleton_class.const_defined?(:SUFFIX)
__END__
"hi!"
true
false
