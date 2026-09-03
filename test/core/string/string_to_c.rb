# String#to_c: lenient leading-substring parse into a Complex.

# Rectangular forms.
p "3+4i".to_c
p "-3-4i".to_c
p "5+i".to_c
p "1.5+2.5i".to_c

# Pure real and pure imaginary.
p "1.5".to_c
p "7".to_c
p "3i".to_c
p "3.0e2i".to_c

# Bare imaginary unit, with optional sign.
p "i".to_c
p "+i".to_c
p "-i".to_c

# Rational components.
p "2/3".to_c
p "2/3+1/4i".to_c
p "1.5/2".to_c

# Polar (magnitude@angle); a zero angle keeps the exact magnitude.
p "1@2".to_c
p "1@0".to_c

# Whitespace is trimmed; junk and empty strings parse to (0+0i).
p "  3+4i  ".to_c
p "foo".to_c
p "".to_c

# Digit-group underscores; a trailing '.' and an unterminated tail stop the parse.
p "1_000".to_c
p "5.".to_c
p "3+4".to_c

# A zero denominator raises, matching Rational parsing.
begin
  "1/0".to_c
rescue ZeroDivisionError => e
  puts "raised: #{e.message}"
end
__END__
(3+4i)
(-3-4i)
(5+1i)
(1.5+2.5i)
(1.5+0i)
(7+0i)
(0+3i)
(0+300.0i)
(0+1i)
(0+1i)
(0-1i)
((2/3)+0i)
((2/3)+(1/4)*i)
((3/4)+0i)
(-0.4161468365471424+0.9092974268256817i)
(1+0.0i)
(3+4i)
(0+0i)
(0+0i)
(1000+0i)
(5+0i)
(3+0i)
raised: divided by 0
