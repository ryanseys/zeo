# Weak references and finalizers: ObjectSpace::WeakMap (identity-keyed, holding
# weak references to its keys and values), WeakRef (a delegator that raises
# once its referent is gone), and ObjectSpace.define_finalizer.
require "weakref"
require "objspace"

# --- ObjectSpace::WeakMap: an identity-keyed weak map ---
# Keys and values are kept alive here by ordinary local variables, so the map
# reads back exactly what was put in; entries would vanish only once their key
# had no other reference anywhere.
cache = ObjectSpace::WeakMap.new
alice = Object.new
bob = Object.new
name_a = "Alice"
name_b = "Bob"
cache[alice] = name_a
cache[bob] = name_b
puts cache[alice]
puts cache.key?(bob)
puts cache.length
puts cache.values.sort.inspect
# Identity keying: a fresh, distinct object is never a member.
puts cache.key?(Object.new)
# delete returns the value and drops the entry.
puts cache.delete(alice)
puts cache.key?(alice)
puts cache.length

# --- WeakRef: delegate to a referent while it lives ---
class Sensor
  def initialize(name) = (@name = name)
  def reading = "#{@name}: 42"
end
sensor = Sensor.new("temp")
ref = WeakRef.new(sensor)
puts ref.reading            # delegated to the referent
puts ref.weakref_alive?
puts ref.respond_to?(:reading)

# Once the referent has been collected, every delegated call raises RefError.
def dangling = WeakRef.new(Object.new)
dead = dangling
GC.start
begin
  dead.reading
rescue WeakRef::RefError => e
  puts "recycled: #{e.message}"
end

# --- define_finalizer: a best-effort callback that runs at program exit ---
keeper = Object.new
ObjectSpace.define_finalizer(keeper, proc { |_id| puts "keeper finalized at exit" })
puts "done"
__END__
Alice
true
2
["Alice", "Bob"]
false
Alice
false
1
temp: 42
true
true
recycled: Invalid Reference - probably recycled
done
keeper finalized at exit
