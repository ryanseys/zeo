# Hash#each_with_object — iterates k,v pairs and threads `memo`
# through the block. The destructured form `|(k,v), memo|` matches
# the MultiTargetNode containing the two names; the flat 3-param
# shape `|k, v, memo|` is also accepted.
#
# Note: an empty `[]` seed is typed as int_array, so a block that
# pushes strings widens incorrectly. Use a typed seed
# (\`[""]\`-style or pop after) when the block produces non-int values.

h = {a: 1, b: 2, c: 3}
result = h.each_with_object([]) { |(k,v), memo| memo << v * 10 }
puts result.inspect

# Flat 3-param form
result2 = h.each_with_object([]) { |k, v, memo| memo << v * 100 }
puts result2.inspect
__END__
[10, 20, 30]
#@ stderr
core/hash/hash_each_with_object.rb:15:in 'block in <main>': undefined method '<<' for nil (NoMethodError)
	from core/hash/hash_each_with_object.rb:15:in 'Hash#each'
	from core/hash/hash_each_with_object.rb:15:in 'Enumerable#each_with_object'
	from core/hash/hash_each_with_object.rb:15:in '<main>'
#@ exit 1
