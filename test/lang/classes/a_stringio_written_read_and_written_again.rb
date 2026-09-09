# Reading #string between two rounds of appends, five times over.
# (spinel issue #3153)
require "stringio"
5.times do
  buf = StringIO.new
  19.times { |i| buf << ("a".."z").to_a[i] }
  _ = buf.string
  19.times { |i| buf << ("a".."z").to_a[i] }
end
puts "ok"
__END__
ok
