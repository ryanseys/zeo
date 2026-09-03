# StringIO's write modes: append mode seeks to the END before each write
# (zeo overwrites from position 0 -- "cb" instead of "abc": data
# corruption, not a missing error), and a read-only StringIO refuses a
# write with IOError (zeo writes and answers the byte count). (Found by
# the 2026-08-24 probe sweep.)
require "stringio"
s = StringIO.new(+"ab", "a")
s.write("c")
p s.string
begin
  StringIO.new("x", "r").write("y")
rescue IOError => e
  puts e.message
end
__END__
"abc"
not opened for writing
