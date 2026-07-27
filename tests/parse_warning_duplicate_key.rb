# Ruby warns at PARSE time when a literal hash key is written twice: the
# second value wins silently, so the warning is the only sign. It names the
# line the surviving value is on, which is not always the line of the key it
# overwrote.
#
# These are the only warnings zeo forwards. They arrive on stderr before any
# program output, which is where CRuby prints them too -- see the sidecar
# golden.

# --- a hash literal ----------------------------------------------------------
p({ k: 1, k: 2 })
p({ "s" => 1, "s" => 2 })
p({ 1 => :a, 1 => :b })

# --- across lines: the warning points at the WINNER ---------------------------
p({
  dup: 1,
  other: 2,
  dup: 3,
})

# --- keyword arguments are the same list ------------------------------------
def capture(**opts) = opts
p capture(a: 1, a: 2)

# --- a splat colliding with a literal pair ----------------------------------
p({ x: 0, **{ x: 9 } })
p capture(**{ y: 1 }, y: 2)

# --- inside a method body, and inside a block -------------------------------
def build = { m: 1, m: 2 }
p build
p [1].map { { b: 1, b: 2 } }

# --- three of the same key warn twice ---------------------------------------
p({ t: 1, t: 2, t: 3 })

# --- keys that only LOOK duplicated do not warn ------------------------------
p({ a: 1, b: 2 })
p({ "a" => 1, a: 2 })
p({ 1 => :int, "1" => :str, :"1" => :sym })
n = 1
p({ n => :first, 2 => :second })
