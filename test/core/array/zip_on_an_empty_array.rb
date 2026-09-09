# It answers an empty array whatever the operands hold, including two of them.
# (spinel issue #3332)
p([].zip(["a"]))
p([].zip([1]))
p([].zip([1.5]))
p([].zip([:a]))
p([].zip([[1]]))
p([].zip(["a"], [2]))
a = []
p a.zip(["x"])
p [1, 2].zip(["a", "b"])
__END__
[]
[]
[]
[]
[]
[]
[]
[[1, "a"], [2, "b"]]
