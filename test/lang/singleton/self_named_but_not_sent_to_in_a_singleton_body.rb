# A statement inside `class << self` that consults `self` only by NAMING it --
# never by sending to an implicit receiver -- needs no compile-time singleton
# class at all. The `self` just has to evaluate to the right object, and
# `self.singleton_class` in the enclosing class body IS that object.
#
# zeo refused the whole shape, because the mapping works by rewriting a
# statement's RECEIVER and this one has no receiver to rewrite.
# `Mongoid.deprecate(self, :from_hash)` is 99 corpus rows of exactly it.
class Sink
  def self.take(x) = x
  def self.pair(a, b) = [a, b]
end

class Foo
  class << self
    Sink.take(self)
  end
end
p Sink.take(Foo.singleton_class).equal?(Foo.singleton_class)

# `self` here is the SINGLETON class, not the class -- which is the whole
# reason passing the statement through unchanged was refused: it would have
# handed over the enclosing class and answered silently wrong.
$seen = []
class Bar
  class << self
    $seen << Sink.take(self)
    def m = :m
  end
end
p $seen.first.equal?(Bar.singleton_class)
p $seen.first.equal?(Bar)
p $seen.first.to_s

# `self` inside a nested `def` in the same body is that method's future
# RECEIVER, not the singleton class, so it must NOT be rewritten.
class Baz
  class << self
    def who = self
    def who_singleton = self.singleton_class
  end
end
p Baz.who.equal?(Baz)
p Baz.who_singleton.equal?(Baz.singleton_class)

# More than one `self` in one statement, and `self` in a nested argument.
class Multi
  class << self
    Sink.pair(self, Sink.take(self))
  end
end
p Multi.singleton_class.to_s

# `self` reached through a node that is NOT a call at all -- an assignment's
# right-hand side. These never matched the plain `self`-receiver rule, which
# only ever looked at a `Call`'s receiver.
$captured = nil
class Assigned
  class << self
    $captured = self
  end
end
p $captured.equal?(Assigned.singleton_class)
p $captured.equal?(Assigned)

# A statement whose `self` sits inside a BLOCK still works: the block runs with
# the class as `self`, so `self.singleton_class` is the same object there.
class Blocky
  class << self
    [1].each { Sink.take(self) }
  end
end
p Blocky.singleton_class.to_s

# And a plain no-self statement beside them is untouched.
class Plain
  class << self
    Sink.take(:no_self_here)
    def q = :q
  end
end
p Plain.q
__END__
true
true
false
"#<Class:Bar>"
true
true
"#<Class:Multi>"
true
false
"#<Class:Blocky>"
:q
