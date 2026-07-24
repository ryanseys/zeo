# Rightward assignment `expr => var` (Ruby 3.0+). Prism encodes this
# as a MatchRequiredNode whose `pattern` is a LocalVariableTargetNode.
# spinel_parse rewrites the simple-target case to a LocalVariableWriteNode
# at the AST boundary so the codegen reuses the regular assignment path.

# 1. Integer rightward assignment.
42 => x
puts x
# 42

# 2. String rightward assignment.
"hello" => msg
puts msg
# hello

# 3. Method-call result rightward-assigned.
def square(n)
  n * n
end

square(7) => sq
puts sq
# 49

# 4. Rightward inside an expression (each statement creates one local).
1 + 2 + 3 => total
total * 2 => doubled
puts total
# 6
puts doubled
# 12

# 5. Boolean.
(5 > 3) => is_bigger
puts is_bigger
# true
