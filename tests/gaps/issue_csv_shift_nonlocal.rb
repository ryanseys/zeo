# `CSV#shift` is `parser.parse { |row| return row }` -- a non-local return out
# of a block yielded from deep inside the parser. zeo answers nil instead of the
# row, so `shift`/`gets`/`readline` never produce anything, while `CSV.parse`
# and `CSV#read` (which collect rather than return early) work.
#
# The plain shape of that return -- a block passed down one `yield` -- does
# work, so the swallow is somewhere in the parser's own chain.
require "csv"
c = CSV.new("a,b\nc,d\n")
p c.shift
p c.shift
p c.shift
p CSV.new("k,v\n").gets
