# Bare String.new, appended to with <<, and with an initial value.

# Bare String.new — empty mutable buffer.
io = String.new
io << "hello"
puts io                   # hello

# Constructor with seed.
s = String.new("foo")
s << "bar"
puts s                    # foobar

# Empty buffer reads as length 0.
t = String.new
puts t.length             # 0

# Canonical Rails-style accumulator.
def render(items)
  io = String.new
  io << "<ul>"
  items.each { |item| io << "<li>" << item << "</li>" }
  io << "</ul>"
  io
end
puts render(["a", "b"])   # <ul><li>a</li><li>b</li></ul>
__END__
hello
foobar
0
<ul><li>a</li><li>b</li></ul>
