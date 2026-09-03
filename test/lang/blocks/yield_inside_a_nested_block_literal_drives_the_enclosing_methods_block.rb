# `yield`/`block_given?` lexically inside a block literal refers to the
# ENCLOSING METHOD's own block (a block has no implicit block of its
# own): `analyze::scan_bare_block_use` counts it, so the method gets its
# `__blk` parameter, and the emitted block captures it (see
# `clif::blocks::build_proc`). Oracle-verified.

class Foo
  def helper(x)
    yield x
  end
  def bar
    helper(1) { yield }
  end
  def baz
    [1, 2].map { |v| yield v }
  end
  def has_block
    [1].each { return block_given? }
  end
end
p(Foo.new.bar { "from-outer" })
p(Foo.new.baz { |v| v * 10 })
p Foo.new.has_block
p(Foo.new.has_block {})
__END__
"from-outer"
[10, 20]
false
true
