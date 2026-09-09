# An array joined with no separator, stored, and read after further
# allocation: the bytes are still there.
# (spinel issue #3151)
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
__END__
50
10
549
ok
