require "stringio"
5.times do
  buf = StringIO.new
  19.times { |i| buf << ("a".."z").to_a[i] }
  _ = buf.string
  19.times { |i| buf << ("a".."z").to_a[i] }
end
puts "ok"
