# The RegexpError message reads "unterminated character class"; ruby
# 4.0.6's Onigmo says "premature end of char-class".
#
# Issue #846: a bad `Regexp.new("...")` raises a catchable
# RegexpError. Pre-fix the static-table compile error fired
# before main()'s setjmp scope and terminated the process.
begin
  r = Regexp.new("[invalid")
  puts "no error raised"
rescue => e
  puts "caught: " + e.class.to_s + ": " + e.message
end
puts "after rescue"

# Valid pattern still compiles.
r2 = Regexp.new("hello")
puts "world hello".match?(r2)
__END__
caught: RegexpError: premature end of char-class: /[invalid/
after rescue
true
