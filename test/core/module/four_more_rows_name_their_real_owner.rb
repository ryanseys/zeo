# Dir#entries comes from Enumerable, and a lazy enumerator's each_cons,
# each_slice and each_with_index from Enumerable and Enumerator.
p Dir.instance_methods(false).include?(:entries), Dir.instance_method(:entries).owner.to_s
LAZY = %i[each_cons each_slice each_with_index].freeze
p (Enumerator::Lazy.instance_methods(false) & LAZY).sort
p LAZY.map { Enumerator::Lazy.instance_method(_1).owner.to_s }
p (1..Float::INFINITY).lazy.each_slice(2).first(2)
p (1..Float::INFINITY).lazy.each_cons(2).first(2)
p (1..4).lazy.each_with_index.to_a
p Dir.new(".").entries.include?(".")
__END__
false
"Enumerable"
[]
["Enumerable", "Enumerable", "Enumerator"]
[[1, 2], [3, 4]]
[[1, 2], [2, 3]]
[[1, 0], [2, 1], [3, 2], [4, 3]]
true
