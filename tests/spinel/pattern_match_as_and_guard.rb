# Issues #884 #905: pattern-match `as` binding and `if` guard
# clause both used to leave the matched LV undeclared, causing
# C compile errors. Now both bind correctly: CapturePatternNode
# binds the as-var to the scrutinee at body entry; IfNode-wrapped
# pattern AND-folds the guard against the inner match.

# #884: array pattern with `=> arr` capture
case [1, 2, 3]
in [1, *] => arr
  puts arr.inspect
end

# #905: guard clause with bound variable
case 10
in n if n > 5
  puts "big: " + n.to_s
in n
  puts "small: " + n.to_s
end

# #905: guard's else branch
case 3
in n if n > 5
  puts "big: " + n.to_s
in n
  puts "small: " + n.to_s
end

# #884: object capture
case 42
in Integer => v
  puts "int: " + v.to_s
end
