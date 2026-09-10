# first and to_a on a lazy select or reject, over an empty array and a two-element one.
p([].lazy.select { |x| true }.first(2))
p([].lazy.reject { |x| false }.to_a)
p([1, 2].lazy.select { |x| true }.first(2))
__END__
[]
[]
[1, 2]
