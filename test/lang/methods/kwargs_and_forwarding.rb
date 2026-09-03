# Keyword arguments through every dispatch shape, plus argument forwarding
# (`...`, anonymous `*`/`**`/`&`).

def config(host:, port: 80, **rest)
  [host, port, rest]
end

# Literal keywords, in any written order.
p config(host: "a")
p config(host: "a", port: 8080)
p config(port: 8080, host: "a")
p config(host: "a", debug: true, retries: 3)

# A `**hash` splat supplies them.
opts = { host: "b", port: 443 }
p config(**opts)
p config(**{ host: "c" })

# Keywords through `send` -- the dynamic path.
p send(:config, host: "d")
p send(:config, **opts)

# Through a Method object.
p method(:config).call(host: "e", port: 1)

# Through a dynamically-typed receiver.
class Server
  def start(host:, port: 3000)
    "#{host}:#{port}"
  end
end
s = Server.new
p s.start(host: "x")
p s.start(**{ host: "y", port: 9 })
holder = [Server.new]
p holder[0].start(host: "z")
p holder.first.start(host: "w", port: 2)

# A missing required keyword raises at runtime, so it can be rescued.
begin
  config(port: 1)
rescue ArgumentError => e
  puts "ArgumentError: #{e.message}"
end

# An unknown keyword raises too (no **rest to absorb it).
def strict(a:) = a
begin
  strict(a: 1, b: 2)
rescue ArgumentError => e
  puts "ArgumentError: #{e.message}"
end

# Keywords and positionals together.
def mixed(a, b = 2, *rest, key:, opt: :o, **kw)
  [a, b, rest, key, opt, kw]
end
p mixed(1, key: :k)
p mixed(1, 9, :r, key: :k, opt: :p, extra: 1)

# A trailing Hash is NOT implicitly converted to keywords (Ruby 3+).
def positional_hash(h) = h
p positional_hash({ a: 1 })

# --- Forwarding ------------------------------------------------------------

def target(a, b = :db, *rest, key: :dk, **kw, &blk)
  out = [a, b, rest, key, kw]
  out << blk.call if blk
  out
end

# `...` forwards positionals, keywords and the block together.
def fwd_all(...) = target(...)
p fwd_all(1)
p fwd_all(1, 2, 3, key: :k, extra: 9)
p(fwd_all(1) { "blk" })

# Anonymous `*` forwards positionals.
def fwd_pos(*) = target(*)
p fwd_pos(1, 2, 3)

# Anonymous `**` forwards keywords.
def fwd_kw(a, **) = target(a, **)
p fwd_kw(1, key: :k, x: 1)

# Anonymous `&` forwards the block.
def fwd_blk(a, &) = target(a, &)
p(fwd_blk(1) { "anon-blk" })

# Named forwarding still works.
def fwd_named(*args, **kw, &blk) = target(*args, **kw, &blk)
p fwd_named(1, 2, key: :k)
p(fwd_named(1) { "named" })

# A splat and literal args compose.
def splat_mix(*args) = target(*args, key: :from_call)
p splat_mix(1, 2)

# Forwarding through two hops.
def hop1(...) = hop2(...)
def hop2(...) = target(...)
p hop1(1, 2, key: :hopped)
__END__
["a", 80, {}]
["a", 8080, {}]
["a", 8080, {}]
["a", 80, {debug: true, retries: 3}]
["b", 443, {}]
["c", 80, {}]
["d", 80, {}]
["b", 443, {}]
["e", 1, {}]
"x:3000"
"y:9"
"z:3000"
"w:2"
ArgumentError: missing keyword: :host
ArgumentError: unknown keyword: :b
[1, 2, [], :k, :o, {}]
[1, 9, [:r], :k, :p, {extra: 1}]
{a: 1}
[1, :db, [], :dk, {}]
[1, 2, [3], :k, {extra: 9}]
[1, :db, [], :dk, {}, "blk"]
[1, 2, [3], :dk, {}]
[1, :db, [], :k, {x: 1}]
[1, :db, [], :dk, {}, "anon-blk"]
[1, 2, [], :k, {}]
[1, :db, [], :dk, {}, "named"]
[1, 2, [], :from_call, {}]
[1, 2, [], :hopped, {}]
