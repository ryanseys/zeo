# An argument with no value -- an unresolved call, which lowers to a raise --
# sitting in a non-final position beside another side-effecting argument. The
# argument-sequencing pass has nothing to sequence it into: a valueless
# expression has no C storage, and it does not return, so no sibling can
# observe it. Left to the argument slot, it coerces to the parameter's type.

def join(a, b)
  a.to_s + b
end

key = "sid"
begin
  puts join(nil.missing_method, "cookie." + key)
rescue NoMethodError
  puts "raised"
end
