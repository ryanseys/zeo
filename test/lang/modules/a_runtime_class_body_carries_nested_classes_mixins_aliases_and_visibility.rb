# A class with a DYNAMIC superclass (`class Foo < base`, base a method) is
# built at runtime; its body can now hold a nested class, `include`, a
# constant, `alias`, and a visibility directive -- each rewritten to the
# runtime self-send the class-value receiver serves (the same machinery
# that lets `class Tempfile < DelegateClass(File)` compile).

def base; Object; end
module Mx; def mixed; "mx"; end; end
class Foo < base
  VAL = 7
  include Mx
  def a; VAL; end
  alias b a
  protected :b
  class Inner
    def deep; "deep"; end
  end
  def use_inner; Inner.new.deep; end
end
puts Foo.new.a
puts Foo.new.use_inner
puts Foo.new.mixed
puts Foo.ancestors.include?(Mx)
__END__
7
deep
mx
true
