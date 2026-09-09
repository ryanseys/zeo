# A built-in exception class's `.new("msg")`: `.class`, `.message`, and
# `is_a?` up the hierarchy. Then raising it, both as a pre-built object and
# as one constructed inline at the raise.

err = RuntimeError.new("created")
puts err.class                     # RuntimeError
puts err.message                   # created
puts err.is_a?(RuntimeError)       # true
puts err.is_a?(StandardError)      # true
puts err.is_a?(Exception)          # true

# raise the saved object
begin
  raise err
rescue => e
  puts "caught: #{e.class}: #{e.message}"
end

# raise an inline-constructed exception (1D)
begin
  raise ArgumentError.new("inline ae")
rescue => e
  puts "inline: #{e.class}: #{e.message}"
end

# StandardError subclass with .new
e2 = TypeError.new("bad type")
puts e2.class                      # TypeError
puts e2.message                    # bad type

# Various built-in exception classes
e3 = NoMethodError.new("missing")
puts e3.class                      # NoMethodError
__END__
RuntimeError
created
true
true
true
caught: RuntimeError: created
inline: ArgumentError: inline ae
TypeError
bad type
NoMethodError
