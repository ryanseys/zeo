# Issue #203: `String.new` resolved to 0 because the constructor
# dispatch had no String arm. Code that combined the result with
# `<<` (the Rails view-emit accumulator pattern) failed to compile.

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
