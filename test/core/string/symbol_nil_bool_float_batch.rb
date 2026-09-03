# Symbol#intern/#name(frozen)/#casecmp(String), boolean & | ^ with Integer,
# clone(freeze:) on immutable values, Float comparison/to_int/coerce edges,
# nil <=> / rationalize / =~ / !~.

# Symbol
p(:hello.intern)
s = :hello
p(s.intern)
p(:hello.name.frozen?)
p(:hello.name)
p(:HELLO.casecmp("hello"))
p(:HELLO.casecmp?("hello"))
p(:abc.casecmp(:ABD))
p(:abc.casecmp?(:ABC))

# true/false & | ^ with an Integer operand (0 is truthy)
p(true & 0)
p(true & 2)
p(false | 0)
p(true ^ 0)
p(false ^ 0)
a = true
b = 0
p(a & b)

# clone(freeze:) on immutable values
p(true.clone.class)
p(true.clone(freeze: true).class)
p(false.clone(freeze: true).class)
p(nil.clone(freeze: true).class)
p(1.clone(freeze: true).class)

# Float comparisons with non-numeric operands raise ArgumentError
f = 1.5
begin
  p(f < nil)
rescue => e
  p e.class
end
begin
  p(f > Object.new)
rescue => e
  p e.class
end

# Float#to_int on Infinity/NaN raises FloatDomainError like to_i
begin
  p(Float::INFINITY.to_int)
rescue => e
  p e.class
end
begin
  p(Float::NAN.to_int)
rescue => e
  p e.class
end
p(3.9.to_int)

# Float#coerce with Rational/Complex arguments
p(1.0.coerce(Rational(1, 2)))
p(2.0.coerce(Complex(1, 0)))
p(1.0.coerce(2))

# nil <=> / rationalize / =~ / !~
p(nil <=> nil)
p(nil <=> 1)
p(nil.rationalize)
p(nil =~ /x/)
p(nil !~ /x/)
__END__
:hello
:hello
true
"hello"
nil
nil
-1
true
true
true
true
false
true
true
TrueClass
TrueClass
FalseClass
NilClass
Integer
ArgumentError
ArgumentError
FloatDomainError
FloatDomainError
3
[0.5, 1.0]
[1.0, 2.0]
[2.0, 1.0]
0
nil
(0/1)
nil
true
