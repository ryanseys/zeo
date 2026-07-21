# Splat in value positions: `break *x` / `next *x` build an array (nil -> [],
# array -> itself, scalar -> [v]); `when *arr` is membership of the splatted
# array (literal or variable), on both statement and value-position cases.
a = while true; break *[1, 2]; end
p a
b = while true; break *nil; end
p b
c = while true; break *1; end
p c
def r(v)
  x = yield
  v == x
end
p r([1]) { next *1 }
p r([]) { next *nil }
p r([2, 3]) { next *[2, 3] }
def cat(x)
  case x
  when *['a', 'b', 'c'] then "letter"
  when *[1, 2, 3] then "number"
  else "other"
  end
end
p cat('b')
p cat(2)
p cat(:z)
lst = [10, 20]
case 20
when *lst then puts "in list"
else puts "not"
end
