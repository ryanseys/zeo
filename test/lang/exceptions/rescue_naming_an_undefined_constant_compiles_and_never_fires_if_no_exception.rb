# The `rescue` clause's class expression is only evaluated while matching
# an actually-raised exception. A clause that never fires must not fail
# the compile just because its class isn't defined.

begin
  1 + 1
rescue NeverDefined
  puts "caught"
end
puts "ok"
__END__
ok
