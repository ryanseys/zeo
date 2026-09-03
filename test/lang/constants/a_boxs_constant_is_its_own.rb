# The CONSTANT channel, both halves. A box's `Object.const_set` is
# unreachable from main, and reachable from the box -- which needs its own
# record for `Object`, since a box's cref chain ends at its surrogate and
# otherwise never consults `Object` at all.
#@ ruby: -W:no-experimental
#@ env: RUBY_BOX=1

b = Ruby::Box.new
b.eval("Object.const_set(:BOXCONST, 5)")
p(begin; Object.const_get(:BOXCONST); rescue NameError; :namee; end)
p b.eval("BOXCONST")
__END__
:namee
5
