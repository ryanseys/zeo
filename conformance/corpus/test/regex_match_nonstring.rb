# `Regexp#=~` (and match?/===) with a non-String operand raises
# TypeError in CRuby; Spinel used to pass the scalar where a char* was
# expected and segfault.
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
