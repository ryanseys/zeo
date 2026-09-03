# The G2 trailing-kwargs-hash convention: `send`'s truly-dynamic
# fallback (non-literal target name) carries kwargs as one trailing
# Hash; the callee's trampoline pops and binds it.

class Foo
  def bar(x:)
    x
  end
end
name = :bar
puts Foo.new.send(name, x: 41) + 1
__END__
42
