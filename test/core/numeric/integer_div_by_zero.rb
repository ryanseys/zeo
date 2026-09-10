# Integer division / modulo / divmod / ceildiv by zero raises
# ZeroDivisionError. Previously these operations triggered C
# undefined behaviour (SIGFPE on x86) outside the longjmp net the
# rescue keyword unwinds — see the now-stale comment in
# test/endless_method_rescue.rb.
#
# The program prints the exception itself rather than `.message`; what it
# checks is that the division raises and the raise is catchable.

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

# Float division by zero is NOT affected: IEEE 754 answers Infinity or NaN
# and nothing is raised.
puts (1.0 / 0.0).infinite?
puts (0.0 / 0.0).nan?
__END__
caught div: divided by 0
caught mod: divided by 0
caught divmod: divided by 0
caught ceildiv: divided by 0
caught powmod: divided by 0
caught /=: divided by 0
caught %=: divided by 0
bare-rescue: divided by 0
1
true
