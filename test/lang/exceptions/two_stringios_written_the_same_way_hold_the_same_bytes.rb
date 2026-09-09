# Building the same string in two StringIOs, five times over, gives equal contents each time.
# (spinel issue #3152)
require "stringio"

5.times do
  buf = StringIO.new
  20.times { |i| buf << ("a".."z").to_a[i % 26] * (i + 1) }
  s = buf.string

  buf2 = StringIO.new
  20.times { |i| buf2 << ("a".."z").to_a[i % 26] * (i + 1) }
  s2 = buf2.string

  raise unless s == s2
end

puts "ok"
__END__
ok
