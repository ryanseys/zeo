# Runtime `Module#alias_method` (computed names, ostruct's bulk-`!` loop
# shape): aliases of user methods, of builtins (`dup`/`class`), snapshot
# semantics against a later runtime redefinition, the Symbol return value,
# NameError for an unresolvable source, and the frozen-class refusal.
# Expected output oracle-verified verbatim.

class T
  def greet
    "hi"
  end
end
class T
  instance_methods(false).each do |m|
    alias_method "#{m}!", m
  end
end
t = T.new
puts t.greet!
class T
  ["dup", "class"].each { |m| alias_method "my_#{m}", m.to_sym }
end
p t.my_class
p t.my_dup.class
class Snap; end
n1 = :v
Snap.class_eval { define_method(n1) { "old" } }
Snap.class_eval do
  [[:v2, :v]].each { |a, b| p alias_method(a, b) }
end
Snap.class_eval { define_method(n1) { "new" } }
o = Snap.new
p o.v2
p o.v
class T
  begin
    [[:x, :nope]].each { |a, b| alias_method(a, b) }
  rescue NameError => e
    puts e.message
  end
end
class Fz
  def m
    1
  end
end
Fz.freeze
begin
  Fz.class_eval { [[:m2, :m]].each { |a, b| alias_method(a, b) } }
rescue FrozenError => e
  puts e.message
end
puts "done"
__END__
hi
T
T
:v2
"old"
"new"
undefined method 'nope' for class 'T'
can't modify frozen Class: Fz
done
