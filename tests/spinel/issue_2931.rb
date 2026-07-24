# A hash pattern in case/in must match a poly scrutinee at runtime (checking
# the value is a hash with the required keys), not fail closed.
def f(v)
  case v
  in [x, y]
    "array #{x},#{y}"
  in { r: }
    "hash r=#{r}"
  else
    "other"
  end
end
p f([3, 4])
p f({ r: 10 })
p f(5)

# value sub-patterns (a literal, a class check) also match on a poly scrutinee
def g(v)
  case v
  in { status: "ok", code: c }
    "ok #{c}"
  in { r: Integer => n }
    "int #{n}"
  else
    "other"
  end
end
p g({ status: "ok", code: 200 })
p g({ r: 42 })
p g({ r: "x" })
