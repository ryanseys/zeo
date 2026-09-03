# Pre-15.2 this was the codebase's one SILENT-wrongness bug: the second
# `class Foo` registered a shadowed duplicate, so `b` never dispatched
# and the original `a` kept winning.

class Foo
  def a
    "first"
  end
end

class Foo
  def b
    "added"
  end

  def a
    "replaced"
  end
end

puts Foo.new.a
puts Foo.new.b
__END__
replaced
added
