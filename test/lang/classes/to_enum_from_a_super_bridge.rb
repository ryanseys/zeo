# `to_enum(:meth)` on the implicit self, inside a method that some subclass
# reaches with `super`. Such a method is emitted a SECOND time, into a bridge
# container where `self` arrives as a plain `RubyValue` rather than the usual
# typed receiver -- and `to_enum`'s own fast path boxed it as though it were
# typed, so the generated program did not compile. prism's `Pattern#scan`
# (`return to_enum(:scan, root) unless block_given?`) is the shape; irb pulls
# in forty-odd of them.
class Base
  def each
    return to_enum(:each) unless block_given?
    yield 1
    yield 2
  end

  def each_tagged(tag)
    return enum_for(:each_tagged, tag) unless block_given?
    yield "#{tag}-a"
    yield "#{tag}-b"
  end
end

class Sub < Base
  def each(&block)
    return to_enum(:each) unless block_given?
    super(&block)
    yield 3
  end
end

p Base.new.each.to_a
p Sub.new.each.to_a
p Base.new.each_tagged("x").to_a

collected = []
Sub.new.each { |n| collected << n }
p collected

e = Sub.new.each
p e.class
p e.next
p e.next
__END__
[1, 2]
[1, 2, 3]
["x-a", "x-b"]
[1, 2, 3]
Enumerator
1
2
