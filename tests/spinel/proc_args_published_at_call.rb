# A Proc's boxed arguments ride ONE global side channel, so publishing them
# belongs to the call rather than to the statement above it. As prelude lines,
# two Proc calls in one expression both published before either ran, and the
# first call read the second's arguments -- which a composed Proc's first stage
# saw as nil, since the publish for its own stage had been cleared (#4059).
strip = ->(s) { s.strip }
down  = ->(s) { s.downcase }
fwd = strip >> down
p fwd.call("  AB  ")
p(fwd.call("  AB  ") == fwd.call("  AB  "))

# two DIFFERENT arguments in one expression: each call has to see its own
add = ->(a, b) { a + b }
p(add.call(1, 2) + add.call(10, 20))
p [add.call(1, 2), add.call(3, 4), add.call(5, 6)]

# a plain (uncomposed) lambda on both sides
up = ->(s) { s.upcase }
p(up.call("a") + up.call("b"))

# nested: the inner call publishes while the outer one is being built
wrap = ->(s) { "[#{s}]" }
p wrap.call(wrap.call("x"))

# three stages composed, called twice in one expression
back = down << strip
p(back.call("  CD  ") == back.call("  CD  "))
p [fwd.call("  EF  "), back.call("  GH  ")]
