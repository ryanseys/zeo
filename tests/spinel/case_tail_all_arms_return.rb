# A tail `case` whose every arm leaves the function has no value of its own,
# so the value path widened it to poly and returned the box from a concretely
# typed function. The if/elsif/else form of the same dispatch always worked
# (#4156). d3 keeps the mixed shape honest: one arm returns, the others carry
# a value, so the case still IS the method's value.
def d1(n)
  case n
  when 1 then return [1, 2]
  when 2 then return [3]
  else return []
  end
end

def d2(n)
  case n
  when 1
    return "one"
  else
    raise ArgumentError, "bad"
  end
end

def d3(n)
  case n
  when 1 then return "a"
  when 2 then "b"
  else "c"
  end
end

def d4(n)
  case n
  when 1 then return 10
  else return 20
  end
end

p d1(1), d1(9)
p d2(1)
begin
  d2(2)
rescue ArgumentError => e
  p e.message
end
p d3(1), d3(2), d3(9)
p d4(1), d4(2)
