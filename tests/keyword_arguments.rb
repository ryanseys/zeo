# Keyword arguments and `**` double-splats: literal `k: v` pairs and `**hash`
# expansions live in ONE ordered list, merged left-to-right with last-key-wins.
# Because Ruby's Hash is insertion-ordered, the order the two are written in is
# observable -- so it has to be preserved end to end.

def capture(**opts) = opts

# --- key order is preserved across a splat/pair boundary --------------------
base = { a: 1, b: 2 }

# A splat written BEFORE a literal pair: the splatted keys come first.
p capture(**base, c: 3)          # {a: 1, b: 2, c: 3}

# ...and AFTER: the literal pair comes first.
p capture(c: 3, **base)          # {c: 3, a: 1, b: 2}

# Interleaved, in exactly the written order.
p capture(**{a: 1}, b: 2, **{c: 3})   # {a: 1, b: 2, c: 3}

# --- more than one `**` per call --------------------------------------------
p capture(**{x: 1}, **{y: 2})    # {x: 1, y: 2}

# A duplicated key takes its LAST value, wherever the collision comes from.
p capture(**{k: 1}, k: 2)        # {k: 2}
p capture(k: 1, **{k: 2})        # {k: 2}

# --- the same rules drive a `{ }` hash literal ------------------------------
h = { a: 1 }
p({ **h, b: 2 })                 # {a: 1, b: 2}
p({ b: 2, **h })                 # {b: 2, a: 1}
p({ x: 0, **{ x: 9 } })          # {x: 9}

# --- `**obj` converts through `to_hash` -------------------------------------
# A non-Hash that defines `to_hash` splats like a hash; a plain object would
# raise TypeError.
class Options
  def to_hash = { timeout: 30, retries: 2 }
end
p capture(**Options.new, verbose: true)   # {timeout: 30, retries: 2, verbose: true}

# --- a runtime-empty `**h` contributes nothing at a CALL site ---------------
# (unlike a hash literal, where `{**{}}` is a real empty hash).
def describe(*positional, **kw)
  [positional, kw]
end
empty = {}
p describe(1, 2, **empty)         # [[1, 2], {}]  -- no phantom trailing hash arg
p({ **empty })                    # {}            -- an explicit empty hash literal

# --- keyword params bind by name; a keyword-less callee gets an options hash -
def greet(name:, greeting: "Hello") = "#{greeting}, #{name}!"
puts greet(name: "Ada")
puts greet(**{ name: "Grace", greeting: "Hi" })

def legacy(opts = {}) = opts       # declares NO keywords
p legacy(mode: :fast, level: 3)    # {mode: :fast, level: 3} -- becomes one hash

# --- `yield` forwards keywords too ------------------------------------------
def with_config
  yield 1, **{ width: 80, height: 24 }
end
with_config { |n, cfg| p [n, cfg] }   # [1, {width: 80, height: 24}]
