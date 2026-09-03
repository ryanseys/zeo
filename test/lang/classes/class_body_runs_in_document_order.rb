# A class body runs WHERE IT IS WRITTEN. zeo already emitted it there for a
# body reached directly from the top level, but the reachability walk deciding
# which bodies those are only descended through a class body and a box scope --
# so a `class` inside any other statement container fell back to the prelude and
# ran ahead of everything, before the `rescue` lexically enclosing it existed.
puts "before"
begin
  class Registry
    Undefined
  end
rescue NameError => e
  puts "caught #{e.message}"
end
puts "after"

# The same, through the other containers a definition is commonly guarded by.
ORDER = []
ORDER << :top
if true
  class Guarded
    ORDER << :guarded
  end
end
ORDER << :after_if

case :go
when :go
  module InCase
    ORDER << :in_case
  end
end
ORDER << :after_case

begin
  begin
    class Nested
      ORDER << :nested
    end
  end
end
ORDER << :after_nested
p ORDER

# A reopen inside a container runs again, in place.
class Counter
  COUNT = []
end
2.times do
  class Counter
    COUNT << COUNT.size
  end
end
p Counter::COUNT

# The class is still REGISTERED whole-program, so it resolves as a constant
# wherever it is named.
p Registry.name
p InCase.instance_of?(Module)
p Guarded.superclass
__END__
before
caught uninitialized constant Registry::Undefined
after
[:top, :guarded, :after_if, :in_case, :after_case, :nested, :after_nested]
[0, 1]
"Registry"
true
Object
