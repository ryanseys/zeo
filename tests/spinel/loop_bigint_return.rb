# A method whose loop-carried local is bigint-promoted (d = d * 2) and then
# returned must have a bigint (sp_Bigint *) return type, not void. Bigint was
# missing from is_scalar_ret and c_type_name, so such methods emitted
# `static void` and any use of the result was a hard C error.
def doublings(n)
  d = 1
  i = 0
  while i < n
    d = d * 2
    i += 1
  end
  d
end
puts doublings(100)

def factorial(n)
  r = 1
  i = 1
  while i <= n
    r = r * i
    i += 1
  end
  r
end
puts factorial(30)
x = doublings(3)
puts(x > 0 ? "positive" : "nonpositive")
