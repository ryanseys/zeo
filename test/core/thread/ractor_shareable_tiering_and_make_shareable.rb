puts Ractor.shareable?(1)
puts Ractor.shareable?(:sym)
puts Ractor.shareable?("mutable")
frozen_str = "frozen".freeze
puts Ractor.shareable?(frozen_str)
arr = [1, 2]
puts Ractor.shareable?(arr)
Ractor.make_shareable(arr)
puts Ractor.shareable?(arr)
puts arr.frozen?
__END__
true
true
false
true
false
true
true
