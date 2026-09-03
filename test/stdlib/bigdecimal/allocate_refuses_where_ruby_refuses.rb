# `Class#allocate` on a native class zeo has no blank value for must RAISE,
# not hand back a generic object.
#
# The generic answer was a name-keyed dynamic object, and a native class's
# rows downcast to their own payload -- so `BigDecimal.allocate` succeeded and
# the next method call panicked on the downcast, taking the whole process with
# it rather than raising. `Marshal.load(Marshal.dump(BigDecimal("1.25")))`
# walked straight into it.
#
# Ruby's answer is `TypeError: allocator undefined for X`, and that is now
# zeo's, message included.

require "bigdecimal"

# `BigDecimal` is the one class in a 52-row differential probe where zeo
# allocated and ruby refused, and it is the one that aborted.
begin
  BigDecimal.allocate
  puts "BigDecimal\tallocated"
rescue TypeError => e
  puts "BigDecimal\t#{e.class}: #{e.message}"
end

# The classes ruby also refuses, so the message shape is checked on more
# than one row.
[Symbol, Integer, Float, Struct, Data, Encoding, Proc, Binding, Thread].each do |k|
  begin
    k.allocate
    puts "#{k}\tallocated"
  rescue TypeError => e
    puts "#{k}\t#{e.class}: #{e.message}"
  rescue NoMethodError
    puts "#{k}\tNoMethodError"
  end
end

# The OTHER refusal, which is a different exception class. Ruby `undef`s
# `allocate` outright on these five, so the name is absent rather than
# raising: `rescue TypeError` catches nothing here.
[Rational, Complex, MatchData, Module].each do |k|
  begin
    k.allocate
    puts "#{k}\tallocated"
  rescue TypeError => e
    puts "#{k}\tTypeError: #{e.message}"
  rescue NoMethodError => e
    puts "#{k}\t#{e.class}: #{e.message}"
  end
end

# A plain runtime class still allocates: the refusal is for NATIVE classes
# with no blank value, not for everything.
Plain = Class.new
puts "Plain\t#{Plain.allocate.class}"
puts "Object\t#{Object.allocate.class}"
puts "String\t#{String.allocate.inspect}"
puts "Array\t#{Array.allocate.inspect}"
puts "Hash\t#{Hash.allocate.inspect}"

# A subclass of a plain runtime class allocates through its ancestor.
Sub = Class.new(Plain)
puts "Sub\t#{Sub.allocate.class}"
__END__
BigDecimal	TypeError: allocator undefined for BigDecimal
Symbol	TypeError: allocator undefined for Symbol
Integer	TypeError: allocator undefined for Integer
Float	TypeError: allocator undefined for Float
Struct	TypeError: allocator undefined for Struct
Data	TypeError: allocator undefined for Data
Encoding	TypeError: allocator undefined for Encoding
Proc	TypeError: allocator undefined for Proc
Binding	TypeError: allocator undefined for Binding
Thread	TypeError: allocator undefined for Thread
Rational	NoMethodError: undefined method 'allocate' for class Rational
Complex	NoMethodError: undefined method 'allocate' for class Complex
MatchData	NoMethodError: undefined method 'allocate' for class MatchData
Module	NoMethodError: undefined method 'allocate' for class Module
Plain	Plain
Object	Object
String	""
Array	[]
Hash	{}
Sub	Sub
