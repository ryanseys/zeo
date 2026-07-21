# `def f(a, **kwargs)` binds extra keyword args into a sym_poly_hash.
def test(a, **kwargs)
  puts a
  puts kwargs.inspect
end

test(1, b: 2, c: 3)

# Just kwargs (no positionals)
def only_kw(**opts)
  puts opts.inspect
end

only_kw(x: 10, y: 20)

# No keyword args at the call site -> empty hash
only_kw

# Mix of explicit keyword + kwrest: explicit gets bound, leftover
# goes to kwargs.
def mixed(a, b:, **rest)
  puts "a=#{a} b=#{b}"
  puts rest.inspect
end

mixed(1, b: 2, c: 3, d: 4)
