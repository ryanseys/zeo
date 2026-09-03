# `x`'s two branches assign DIFFERENT classes, so
# `analyze::locals::merge_locals` widens its whole-scope type to `Poly`
# (a plain `RubyValue` slot) -- but each branch's own `HirNode::New`
# codegen still produces a bare, unboxed `Arc<Concrete>` unless the
# write site itself boxes it. Before this fix, this was
# a genuine `rustc` type-mismatch in the GENERATED Rust, not just a
# wrong answer.

class Foo
  def initialize
    @tag = "foo"
  end
  def tag
    @tag
  end
end

class Bar
  def initialize
    @tag = "bar"
  end
  def tag
    @tag
  end
end

class Picker
  def pick(flag)
    if flag
      x = Foo.new
    else
      x = Bar.new
    end
    x
  end
end

p = Picker.new
puts p.pick(true).tag
puts p.pick(false).tag
__END__
foo
bar
