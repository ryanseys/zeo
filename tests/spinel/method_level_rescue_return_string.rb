# Method-level rescue return value with string-returning bodies.
# The int-returning shape works once compile_body_into /
# compile_rescue_chain_into capture the begin / rescue last expr.
# The string-returning shape additionally needs the method's
# inferred return type to unify across both branches, which
# requires BeginNode in infer_type's arms.

# Both branches return string — unify_return_type picks string.
def fetch(s)
  raise "empty" if s == ""
  "got " + s
rescue
  "default"
end
puts fetch("data")
puts fetch("")

# Rescue body with multiple statements. Last is the value.
def parse(s)
  raise "blank" if s == ""
  "len=" + s.length.to_s
rescue
  msg = "fallback"
  msg + "!"
end
puts parse("hi")
puts parse("")

# Method body that's a BeginNode at the top, with a return inside
# the begin. collect_return_types must walk the begin body for the
# explicit `return X` to contribute to the unified type.
def explicit(s)
  raise "neg" if s == ""
  return "early " + s
rescue
  "late"
end
puts explicit("yes")
puts explicit("")
