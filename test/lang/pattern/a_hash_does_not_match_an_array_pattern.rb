# A Hash has no deconstruct, so `in [x, y]` misses and the hash arm takes it.
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
__END__
"array 3,4"
"hash 10"
"hash 9"
"array [1, 2],[3, 4]"
"other"
