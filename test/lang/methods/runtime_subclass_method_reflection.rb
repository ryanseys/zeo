# `Method`-object reflection on a RUNTIME-minted subclass finds the
# inherited frozen class methods -- rspec grabs
# `example_group_class.method(:next_runnable_index_for)` on every
# describe-created group, and the responds/reflection walk only consulted
# the receiver's own (nonexistent) registry entry.
class Base
  def self.next_index(f) = "idx #{f}"
end
k = Class.new(Base)
p k.respond_to?(:next_index)
m = k.method(:next_index)
p m.call(1)
__END__
true
"idx 1"
