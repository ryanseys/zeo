# StringIO's mode string decides what it allows, and the whole table is
# here because every row of it was previously ignored.
#
# Append starting at position 0 was DATA CORRUPTION, not a missing error:
# `StringIO.new("ab", "a").write("c")` answered `"cb"`. Three more rows are
# surprising enough to pin: `"a"` is NOT readable, `"w"` truncates on open,
# and append ignores `pos` entirely -- every write goes to the end however
# the position was set.

require "stringio"
def show
  p yield
rescue Exception => e
  puts "#{e.class}: #{e.message}"
end
show { s = StringIO.new(+"abc", "a"); s.write("d"); s.string }
show { StringIO.new(+"abc", "a").read }
show { StringIO.new(+"abc", "w").string }
show { s = StringIO.new(+"abc", "w"); s.write("z"); s.string }
show { StringIO.new(+"abc", "r").write("y") }
show { StringIO.new(+"abc", "r").read }
show { s = StringIO.new(+"abc", "r+"); s.write("z"); s.string }
show { s = StringIO.new(+"abc", "a+"); s.write("d"); [s.string, (s.rewind; s.read)] }
show { StringIO.new("frozen", "w") }
show { StringIO.new("frozen", "r").read }
show { s = StringIO.new(+"abc", "r"); s.puts("x") }
show { s = StringIO.new(+"abc", "r"); s.print("x") }
show { s = StringIO.new(+"abc", "r"); s << "x" }
show { s = StringIO.new(+"abc", "r"); s.truncate(1) }
show { s = StringIO.new(+"abc"); s.write("z"); s.string }
