# An extreme negative index on `[]=` raises IndexError. Adding the length to
# it leaves it negative, and that is a raise rather than a silent no-op.
a = [1, 2, 3]
begin
  a[-999] = 99
rescue IndexError => e
  puts "int: " + e.message
end
puts a.inspect

b = ["a", "b", "c"]
begin
  b[-50] = "x"
rescue IndexError => e
  puts "str: " + e.message
end
puts b.inspect

# Valid negative index still works.
a[-1] = 33
puts a.inspect
__END__
int: index -999 too small for array; minimum: -3
[1, 2, 3]
str: index -50 too small for array; minimum: -3
["a", "b", "c"]
[1, 2, 33]
