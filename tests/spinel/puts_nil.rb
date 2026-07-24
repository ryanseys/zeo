# `puts nil` (a nil literal) and `puts nil.itself` print a single empty
# line, not `0`. (A nil-*typed* method/ivar that carries a real value is
# deliberately left on the value path, so this gates on the syntactic
# form, not the inferred type.)
puts nil
puts nil.itself
puts "end"
