# A proc's own &block param (#2648): the caller's literal block rides the
# _sp_proc_blk side-channel into the callee's prologue; calling without a
# block binds nil.
p(->(&b) { b.call(9) }.call { |x| x + 1 })
f = ->(&b) { b.nil? ? "none" : b.call(1) }
p f.call
p(f.call { |x| x * 100 })
g = ->(a, &b) { a + b.call(a) }
p(g.call(5) { |x| x * 2 })
# the passed block captures an enclosing local (lifted like any escaping block)
base = 50
p(->(&b) { b.call(3) }.call { |x| base + x })
