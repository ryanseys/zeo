# A user class owning `merge` replaces the whole poly dispatch with its arms,
# so a Hash arriving at the same call matched nothing and raised NoMethodError
# naming its own class. And where that user arm was ALSO dropped because its
# parameter is a concretely different hash kind, the switch came out EMPTY and
# the call quietly answered the result temp's zero initializer (#4033).
class Flash
  def merge(other)
    other.size
  end
end

def pick(f) = f ? {a: 1} : Flash.new

p pick(true).merge(b: 2)
p pick(true).merge({b: 2})
p pick(false).merge({"x" => "y"})
p pick(true).merge({a: 9})
p pick(true).merge(b: 2, c: 3)

# a hash whose keys are not symbols, through the same dispatch
def pick2(f) = f ? {"a" => 1} : Flash.new
p pick2(true).merge({"b" => 2})

# and the ordinary typed receivers, unchanged
p({a: 1}.merge(b: 2))
p({"a" => 1}.merge({"b" => 2}))
p Flash.new.merge({"a" => "b"})
