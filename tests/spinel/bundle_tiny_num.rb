# Bundled tiny tests (auto-grouped by bundler):
#   - float_infinite_nil
#   - float_next_prev
#   - float_zero_p
#   - int_step_expr_block
#   - integer_nonzero
#   - range_size_exclusive
#   - range_size_string

# === float_infinite_nil ===
def t_float_infinite_nil
# Float#infinite?: nil for a finite value, -1 / +1 for -Inf / +Inf.
p 0.0.infinite?
p 3.14.infinite?
p (1.0 / 0.0).infinite?
p (-1.0 / 0.0).infinite?
puts((1.0 / 0.0).infinite? ? "inf" : "finite")
puts(2.5.infinite? ? "inf" : "finite")
end
t_float_infinite_nil

# === float_next_prev ===
def t_float_next_prev
# Float#next_float / prev_float — adjacent IEEE 754 representable
# values via libm's nextafter.
puts 1.0.next_float
puts 1.0.prev_float
puts 0.0.next_float
puts 0.0.prev_float
end
t_float_next_prev

# === float_zero_p ===
def t_float_zero_p
# Float#zero?: true only for 0.0 / -0.0.
p 0.0.zero?
p(-0.0.zero?)
p 3.5.zero?
p (1.0 - 1.0).zero?
puts(0.0.zero? ? "z" : "nz")
end
t_float_zero_p

# === int_step_expr_block ===
def t_int_step_expr_block
# Integer#step with a block in expression position — CRuby returns
# the receiver, not 0. Previously fell through to the unresolved-
# call path and emitted 0 (and analyze inferred int_array, which
# then SEGV'd when result was used as a pointer).
result = 1.step(5) { |x| x }
puts result.inspect
puts result.class
end
t_int_step_expr_block

# === integer_nonzero ===
def t_integer_nonzero
# Issue #874: Integer#nonzero? returns nil when receiver is 0,
# self otherwise. Surfaces as int? (nullable int).
puts 0.nonzero?.inspect
puts 42.nonzero?.inspect
puts(-5.nonzero? || "default")
end
t_integer_nonzero

# === range_size_exclusive ===
def t_range_size_exclusive
# Range#size with exclusive range
# (1...5).size should return 4, not 5.

puts (1..5).size
puts (1...5).size
end
t_range_size_exclusive

# === range_size_string ===
def t_range_size_string
# A String range has no integer size; CRuby Range#size returns nil.
# Integer ranges keep returning their element count.
p (1..10).size
p (1...10).size
p ("a".."z").size
p ("a".."z").size.nil?
p ("a".."e").size
end
t_range_size_string

