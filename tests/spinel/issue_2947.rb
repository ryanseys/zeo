# A Hash must not match an array pattern `[x, y]` (a Hash has no #deconstruct
# in CRuby); the array-pattern condition requires an array-kind poly value.
def f(v)
  case v
  in [x, y] then "array #{x},#{y}"
  in { r: } then "hash #{r}"
  else "other"
  end
end
p f([3, 4])
p f({ a: 1, r: 10 })
p f({ r: 9 })
p f([[1, 2], [3, 4]])
p f("str")
