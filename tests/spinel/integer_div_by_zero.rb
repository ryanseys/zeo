# Integer division / modulo / divmod / ceildiv by zero raises
# ZeroDivisionError. Previously these operations triggered C
# undefined behaviour (SIGFPE on x86) outside the longjmp net the
# rescue keyword unwinds — see the now-stale comment in
# test/endless_method_rescue.rb.
#
# Test uses `puts e` directly (which prints the message string in
# spinel) rather than e.message — the .message dispatch lives on a
# separate exception-bindings PR. The semantic test (raises and is
# catchable) is independent.

# Bare / catches as ZeroDivisionError
begin
  x = 10 / 0
  puts "no raise: #{x}"
rescue ZeroDivisionError => e
  puts "caught div: #{e}"
end

# Bare % catches as ZeroDivisionError
begin
  x = 10 % 0
  puts "no raise: #{x}"
rescue ZeroDivisionError => e
  puts "caught mod: #{e}"
end

# divmod (covers the inline-/ in compile_int_method_expr)
begin
  10.divmod(0)
  puts "no raise"
rescue ZeroDivisionError => e
  puts "caught divmod: #{e}"
end

# ceildiv (was silently returning 0)
begin
  x = 10.ceildiv(0)
  puts "no raise: #{x}"
rescue ZeroDivisionError => e
  puts "caught ceildiv: #{e}"
end

# pow with mod=0 (was silently returning 0)
begin
  x = 2.pow(10, 0)
  puts "no raise: #{x}"
rescue ZeroDivisionError => e
  puts "caught powmod: #{e}"
end

# /= compound assignment
begin
  x = 10
  x /= 0
  puts "no raise: #{x}"
rescue ZeroDivisionError => e
  puts "caught /=: #{e}"
end

# %= compound assignment
begin
  x = 10
  x %= 0
  puts "no raise: #{x}"
rescue ZeroDivisionError => e
  puts "caught %=: #{e}"
end

# Bare rescue also catches (no class filter at all).
begin
  10 / 0
rescue => e
  puts "bare-rescue: #{e}"
end

# Float division by zero is NOT affected — IEEE 754 returns
# Infinity / NaN. Spinel matches CRuby; no exception is raised.
puts (1.0 / 0.0).infinite?
puts (0.0 / 0.0).nan?
