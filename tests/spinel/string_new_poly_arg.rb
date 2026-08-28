# String.new(x) where x is a poly value -- an element read out of a nested
# array, rather than a flat array, a local, or a literal -- previously failed
# C compilation: the argument reached sp_str_dup_external (which takes a
# plain `const char *`) still boxed as an sp_RbVal.
nested = [["class", "x"]]
p String.new(nested[0][1])             # "x"

# A flat array element already worked; keep it covered alongside the nested
# case so a future regression shows up as a diff, not a silent narrowing.
flat = ["class", "y"]
p String.new(flat[1])                  # "y"

# The copy is independent of the source.
def mutable_from_nested(pairs)
  s = String.new(pairs[0][1])
  s << "!"
  s
end
p mutable_from_nested([["k", "z"]])    # "z!"
p [["k", "z"]][0][1]                   # "z" (unchanged)

# A poly value that cannot convert to a String still raises TypeError, same as
# a plain String.new(nil) / String.new(5). A variable index into a
# heterogeneous array keeps the element poly-typed rather than narrowed.
mixed = [nil, 5, "z"]
def poly_at(arr, i); arr[i]; end
begin
  String.new(poly_at(mixed, 0))
rescue TypeError => e
  puts e.message
end
begin
  String.new(poly_at(mixed, 1))
rescue TypeError => e
  puts e.message
end
