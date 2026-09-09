# It answers an empty array, for Integer and String elements, beside the form
# with indices.
# (spinel issue #2980)
p([1, 2, 3].values_at)
p(["a", "b"].values_at)
p([1, 2, 3].values_at(0, 2))
__END__
[]
[]
[1, 3]
