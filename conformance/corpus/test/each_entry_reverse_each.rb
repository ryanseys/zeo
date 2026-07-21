# Enumerable#each_entry alias of each; Enumerable#reverse_each
# walks the receiver in reverse.

[1, 2, 3].each_entry { |x| puts x }
puts "---"
[1, 2, 3].reverse_each { |x| puts x }
puts "---"
["a", "b", "c"].reverse_each { |s| puts s }
