# Two GC bugs behind a StringIO/wrap program:
#  1. a no-separator join emitted a bare "" separator with no 0xff marker, so
#     GC's string mark read one byte before it.
#  2. Array#join returned the transient sp_String builder's buffer; once stored
#     and outliving a GC, the swept builder freed it, leaving a dangling ptr.
def build(chars)
  buf = []
  chars.each { |c| buf << c }
  buf.join            # no separator; result stored below and re-joined later
end

lines = []
50.times do |i|
  chars = []
  10.times { |k| chars << ((97 + (i + k) % 26).chr) }
  lines << build(chars)     # force many allocations between store and use
  ("a".."z").to_a          # churn the GC (String#succ)
end
p lines.length
p lines.first.length
p lines.join("|").length
puts "ok"
