# A block with an EMPTY body still binds its parameters, so a parameter that
# shadows an outer local still needs its own slot. The rename pass skipped such
# a block outright -- there is nothing inside it to rewrite -- and the two names
# then shared one C slot, which took the inner block's type while the outer
# block kept assigning its own (#4027):
#
#   error: assigning to 'sp_PolyArray *' from incompatible type 'sp_RbVal'

# the report's shape: the inner parameter is an array, the outer one a poly
# element of the flat_map source
[].flat_map do |a|
  [].map{}.tap do |a|
  end
end
p [1, 2].flat_map { |a| [3, 4].map { |x| x }.tap { |a| }.size.times.to_a }

# an empty-bodied block whose parameter shadows an outer local of another type
s = "outer"
[7].each do |s|
end
p s
n = 5
%w[a b].each_with_index do |n, i|
end
p n

# the same for a rest parameter and for a block-local, which are renamed on the
# same pass
r = 1
[[8, 9]].each do |*r|
end
p r
q = "keep"
[1].each do |x; q|
end
p q

# the empty block's own value is still nil, and tap still answers its receiver
p([1].each { |z| })
p([5, 6].tap { |z| })
p([1, 2].map { |z| })
