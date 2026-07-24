# Array#assoc / #rassoc on poly_array. Each element is itself a
# sub-array; assoc(key) returns the sub-array whose first element
# matches key, rassoc(val) matches against the second element.

data = [[1, "one"], [2, "two"], [3, "three"]]
puts data.assoc(2).inspect
puts data.rassoc("three").inspect
# Miss → nil.
puts data.assoc(99).inspect
