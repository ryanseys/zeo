# `Regexp#=~`, and match? and === beside it, raise TypeError when the operand
# is not a String.
x = 5
begin
  if /p/ =~ x
    puts "matched"
  else
    puts "no match"
  end
  puts "NO RAISE"
rescue TypeError => e
  puts "TE: " + e.message
end

# A real String operand still matches and returns the offset (or nil).
puts(/l/ =~ "hello")
p(/zz/ =~ "hello")
__END__
TE: no implicit conversion of Integer into String
2
nil
