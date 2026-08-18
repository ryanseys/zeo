require "stringio"

s = StringIO.new("hello")
p s.read(2)
p s.ungetbyte(65)
p s.read
