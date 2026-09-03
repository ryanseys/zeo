# `undef :close` inside a singleton body retires the name for ONE object. It
# had no arm in the per-object desugar and fell through to the pass-through
# catch-all (an `undef` names no `self`), which put a definition-level node
# where a value belongs -- so zeo refused the whole program with "a
# definition-level construct used as a VALUE: undef". The refusal was right
# while there was nowhere for the retirement to be recorded; there is a
# per-object tombstone now.
#
# `class << self` written in a METHOD body opens the singleton of the INSTANCE
# the method was called on. logging's `def kill` is exactly this, and it is the
# largest single cluster in the corpus's lowering-gap bucket.
class Foo
  def close = "class"
  def read = "read"
  def flush = "flush"

  def kill
    class << self
      undef :close
    end
  end
end

g = Foo.new
g.kill
p g.respond_to?(:close)
p(begin
  g.close
rescue NoMethodError
  :raised
end)
p g.read
p Foo.new.close

# The `class << obj` spelling of the same thing, and the multi-name form:
# `undef a, b` names them in one statement and `undef_method` takes them in one
# call.
a = Foo.new
class << a
  undef :close, :read
end
p a.respond_to?(:close), a.respond_to?(:read), a.flush

# An `alias` in a singleton body copies the definition the RECEIVER would have
# run, and the copy survives the original's retirement. zeo wrote the copy into
# an instance table the singleton id never had, where no send for that object
# looks -- so `c.shut` raised where ruby answers.
c = Foo.new
class << c
  alias shut close
  undef :close
end
p c.shut
p(begin
  c.close
rescue NoMethodError
  :raised
end)
p Foo.new.close
__END__
false
:raised
"read"
"class"
false
false
"flush"
"class"
:raised
"class"
