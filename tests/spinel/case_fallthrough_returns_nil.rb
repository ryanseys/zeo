# Issue #707: case without `else` returns nil on fallthrough.
puts (case 99; when 1; "one"; end).inspect
puts (case 1; when 1; "one"; end).inspect
puts (case "x"; when "a"; 42; end).inspect
puts (case "a"; when "a"; 42; end).inspect

# With else clause - unchanged behavior.
puts (case 99; when 1; "one"; else; "other"; end)
