# A parameter that is BOTH captured by an escaping block AND reassigned
# used to get its capture cell declared twice -- once by the prologue's
# param wrap and once by the hoist prelude's param-seeded arm -- so the
# second declaration wrapped the first cell in another cell (invalid
# Rust, from the timeout gem's captured `message ||= ...`). Exactly one
# owner now: the prelude for assigned params, the prologue otherwise.

def f(x)
  t = proc { x }
  x = 2
  t.call
end
p f(1)
def g(m = nil)
  m ||= "d"
  t = proc { m }
  t.call
end
p g
p g("y")
__END__
2
"d"
"y"
