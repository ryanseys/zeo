# Lazy's `_enumerable_*` privates: ruby snapshots the EAGER Enumerable
# bodies onto Lazy before the lazy overrides land, then makes them private.
lz = (1..5).lazy

# Private, present, and correctly counted.
p lz.class.private_instance_methods(false).grep(/\A_enumerable_/).sort.size
p lz.respond_to?(:_enumerable_map)
p lz.respond_to?(:_enumerable_map, true)

# Each drives the EAGER body: an Array comes back, the chain runs at once.
p lz.send(:_enumerable_map) { |x| x * 2 }
p lz.send(:_enumerable_select, &:even?)
p lz.send(:_enumerable_take, 2)
p lz.send(:_enumerable_drop, 3)
p lz.send(:_enumerable_grep, 2..3)
p lz.send(:_enumerable_uniq)
p lz.send(:_enumerable_zip, [9, 8, 7, 6, 5])
p lz.send(:_enumerable_filter_map) { |x| x * 10 if x.odd? }

# The odd one out snapshots Enumerator#with_index.
p lz.send(:_enumerable_with_index, 10).to_a.first(2)

# Visibility: a PUBLIC send refuses.
begin
  lz.public_send(:_enumerable_map) { |x| x }
rescue NoMethodError => e
  puts e.message.split("'").first + "'_enumerable_map'"
end
__END__
18
false
true
[2, 4, 6, 8, 10]
[2, 4]
[1, 2]
[4, 5]
[2, 3]
[1, 2, 3, 4, 5]
[[1, 9], [2, 8], [3, 7], [4, 6], [5, 5]]
[10, 30, 50]
[[1, 10], [2, 11]]
private method '_enumerable_map'
