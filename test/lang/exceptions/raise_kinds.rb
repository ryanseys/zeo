# `raise` accepts several operand shapes: a message string raises a
# RuntimeError, an exception class (with an optional message) raises an
# instance of it, and any NON-exception value is a TypeError.
def try
  yield
rescue => e
  puts "#{e.class}: #{e.message}"
end

try { raise "boom" }                # RuntimeError: boom
try { raise ArgumentError, "bad" }  # ArgumentError: bad
try { raise ArgumentError }         # ArgumentError: ArgumentError
try { raise 42 }                    # TypeError: exception class/object expected
try { raise nil }                   # TypeError: exception class/object expected
try { raise [1, 2] }                # TypeError: exception class/object expected
try { raise :nope }                 # TypeError: exception class/object expected

# The operand can be a runtime-typed (poly) value; raise dispatches on its
# actual class.
things = ["late", 99]
try { raise things[0] }             # RuntimeError: late
try { raise things[1] }             # TypeError: exception class/object expected
__END__
RuntimeError: boom
ArgumentError: bad
ArgumentError: ArgumentError
TypeError: exception class/object expected
TypeError: exception class/object expected
TypeError: exception class/object expected
TypeError: exception class/object expected
RuntimeError: late
TypeError: exception class/object expected
