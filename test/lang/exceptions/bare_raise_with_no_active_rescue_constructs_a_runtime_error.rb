# Bare `raise` (re-raise) outside any active `rescue` clause -- real
# Ruby constructs a fresh `RuntimeError` with an EMPTY message rather
# than erroring (oracle-verified: `ruby -e 'begin; raise; rescue => e;
# puts "[#{e.message}]"; end'` -> `"[]"`) -- see the emitter's raise
# lowering in `clif::stmt`. This used to be a clean compiler
# panic before the `zeo_rt::current_exception` fallback shipped.

begin
  raise
rescue => e
  puts "[#{e.message}]"
end
__END__
[]
