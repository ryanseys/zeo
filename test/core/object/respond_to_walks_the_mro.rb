# `respond_to?` walks the real MRO now: Enumerable names answer true on
# arrays, Comparable names on strings, and Kernel privates stay invisible.

puts [1, 2].respond_to?(:map)
puts "s".respond_to?(:between?)
puts 5.respond_to?(:puts)
puts 5.respond_to?(:itself)
puts "s".respond_to?(:nope)
__END__
true
true
false
true
false
