# CRuby error-protocol edges for numeric and collection operations.
def caught
  yield
rescue => e
  "#{e.class}: #{e.message}"
end

# Float#% by zero raises ZeroDivisionError (int or float divisor), rather than
# answering NaN; a nonzero divisor keeps the floored-modulo sign rule.
p caught { 5.0 % 0 }
p caught { 5.0 % 0.0 }
p 5.0 % 2
p(-5.5 % 2)
p 5.5 % -2

# A negative first/last count raises ArgumentError with the receiver-specific
# message: Array, Range, and the generic Enumerable/Enumerator forms differ.
p caught { [1, 2, 3].first(-1) }
p caught { [1, 2, 3].last(-2) }
p caught { (1..3).first(-1) }
p caught { [1, 2, 3].cycle.first(-1) }
p caught { [1, 2, 3].cycle.take(-2) }

# The non-negative forms are unchanged.
p [1, 2, 3].first(2)
p [1, 2, 3].last(2)
p [1, 2, 3].cycle.first(5)
