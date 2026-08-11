# `raise(...)` -- argument forwarding into raise. The static raise desugar
# has no shape for `...`, so it must route through the general call
# lowering, which expands the forwarding into `*rest, **kw, &blk` and
# reaches the runtime `Kernel#raise` row (sus forwards a matcher's whole
# failure this way).
def reraise(...)
  raise(...)
end

begin
  reraise(ArgumentError, "forwarded")
rescue ArgumentError => e
  puts e.message
end

begin
  reraise("plain message")
rescue RuntimeError => e
  puts e.message
end

# The splat spelling stays on the same path.
def splat_raise(*exc)
  raise(*exc)
end

begin
  splat_raise(TypeError, "splatted")
rescue TypeError => e
  puts e.message
end
