# A container literal builds itself in the statement prelude, so an emitter that
# writes a prelude line and renders an expression straight into it spliced the
# literal's construction into the middle of that line -- not C at all. The
# receiver of a poly `.call` is where this showed: a table of lambdas written
# where it is used, indexed and called in one expression (#4065).
def render(style, value)
  { plain: ->(v) { v.to_s }, quoted: ->(v) { "\"#{v}\"" } }[style].call(value)
end
puts render(:plain, 42)
puts render(:quoted, "hi")

# the Array form of the same shape
def pick(i, v)
  [->(x) { x * 2 }, ->(x) { x + 100 }][i].call(v)
end
p pick(0, 21)
p pick(1, 21)

# a string-keyed table, and one whose values are method objects
def by_name(n)
  { "up" => ->(s) { s.upcase }, "down" => ->(s) { s.downcase } }[n].call("MiXeD")
end
p by_name("up")
p by_name("down")

# nested one level deeper
def nested(k)
  { outer: { inner: ->(x) { x - 1 } } }[k][:inner].call(10)
end
p nested(:outer)
