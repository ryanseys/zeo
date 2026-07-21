# Issue #729. `"#{stmt1; stmt2; ...; stmtN}"` should evaluate every
# statement in source order (for side effects) and use the LAST
# statement's value in the string. spinel used to take stmts.first,
# silently dropping later statements -- losing both their effects on
# locals and the correct interpolation value.

y = 0
s = "#{y = 10; y = 20; y}"
puts s
puts y

# Side effect persists outside the interpolation.
counter = 0
"#{counter += 1; counter += 1; counter += 1}"
puts counter
