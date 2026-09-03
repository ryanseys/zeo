# The hierarchy is live in `is_a?`/`kind_of?`/`instance_of?` -- statically
# folded sites AND the runtime path through a Poly receiver, plus a user
# class inheriting the full Object tail. Every line oracle-verified.

class Widget; end

puts 5.is_a?(Comparable)
puts 5.is_a?(Numeric)
puts 5.is_a?(BasicObject)
puts 3.14.is_a?(Numeric)
puts "s".is_a?(Comparable)
puts [].is_a?(Kernel)
puts nil.is_a?(BasicObject)
puts 5.kind_of?(Comparable)
puts 5.instance_of?(Numeric)
puts [5].first.is_a?(Numeric)
puts Widget.new.is_a?(Kernel)
puts Widget.ancestors.inspect
__END__
true
true
true
true
true
true
true
true
false
true
true
[Widget, Object, Kernel, BasicObject]
