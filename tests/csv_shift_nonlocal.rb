# `CSV#shift` is `parser.parse { |row| return row }` -- a non-local return out
# of a block yielded from deep inside the parser, past frames that catch a
# `Signal::Return` of their own. It is the reason `shift`/`gets`/`readline`
# exist at all, so it is pinned here as well as in
# `nonlocal_return_through_frames.rb`, which covers the shape in isolation.
require "csv"

c = CSV.new("a,b\nc,d\n")
p c.shift
p c.shift
p c.shift
p CSV.new("k,v\n").gets
p CSV.new("m,n\n").readline

# The collecting forms, which never return early, still agree.
p CSV.parse("a,b\nc,d\n")
p CSV.new("e,f\n").read
p CSV.new("g,h\ni,j\n").each.to_a

# `shift` and `each` share the parser's position.
mixed = CSV.new("1,2\n3,4\n5,6\n")
p mixed.shift
rest = []
mixed.each { |row| rest << row }
p rest
p mixed.shift
