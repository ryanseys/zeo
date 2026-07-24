# Float#divmod(0) / Float#divmod(0.0) raises ZeroDivisionError.
#
# Sibling of Integer#divmod(0) which was fixed in the parent
# rescue/raise overhaul (test/integer_div_by_zero.rb). Without
# this guard, `(mrb_int)floor(rc / 0.0)` would consume an
# Infinity/NaN and produce undefined C behaviour at the cast.
#
# Test uses `puts e` directly (which prints the message string in
# spinel) — exception object methods like .message live on a
# separate exception-bindings PR.

# Sanity: divmod with non-zero divisor still works.
q, r = 5.0.divmod(2.0)
puts q
puts r

# Float / int divisor
begin
  1.0.divmod(0)
  puts "no raise"
rescue ZeroDivisionError => e
  puts "caught int divisor: #{e}"
end

# Float / float divisor
begin
  1.0.divmod(0.0)
  puts "no raise"
rescue ZeroDivisionError => e
  puts "caught float divisor: #{e}"
end

# Bare rescue catches it.
begin
  3.5.divmod(0.0)
rescue => e
  puts "bare-rescue: #{e}"
end

# NaN divisor → FloatDomainError("NaN"). Without this guard the
# `(mrb_int)floor(1.0 / NaN)` cast on the result would also be
# C undefined behaviour.
nan = 0.0 / 0.0
begin
  1.0.divmod(nan)
  puts "no raise nan divisor"
rescue FloatDomainError => e
  puts "caught nan divisor: #{e}"
end

# NaN dividend → FloatDomainError("NaN"). Same UB risk on the cast.
begin
  nan.divmod(2.0)
  puts "no raise nan dividend"
rescue FloatDomainError => e
  puts "caught nan dividend: #{e}"
end
