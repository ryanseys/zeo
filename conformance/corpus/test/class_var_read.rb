# ClassVariableReadNode -- `@@var`, plus expression-form
# ClassVariableWriteNode (the `@@x = ...` last-expression-as-return
# path) and several real-world class-var idioms.
#
# Spinel's class vars are independent per declaring class -- the
# ClassVariableWriteNode commit covers the storage-shape
# limitation in detail. This test exercises only patterns that
# stay inside one class's slot (no cross-hierarchy reads).

# 1. Singleton-counter idiom: class-method bumps a shared counter,
#    instance-method reads it. Both routes touch @@count via the
#    same @current_class_idx, so they share the cvar_Counter_count
#    slot.
class Counter
  @@count = 0

  def self.tick
    @@count = @@count + 1   # expression-form ClassVariableWriteNode
  end

  def self.value
    @@count                  # plain ClassVariableReadNode
  end

  def instance_value
    @@count                  # readable from instance methods too
  end
end

puts Counter.value           # 0
Counter.tick
Counter.tick
Counter.tick
puts Counter.value           # 3
puts Counter.new.instance_value   # 3 -- instance reads same slot


# 2. Per-class isolation: two unrelated classes with the same cvar
#    name get independent slots. Without the per-class qualification
#    in cvar_qname, Counter.tick would have changed Other's count
#    too.
class Other
  @@count = 100

  def self.value
    @@count
  end
end

puts Other.value             # 100, unaffected by Counter.tick calls


# 3. Cvar with explicit non-zero initial value at class body. Spinel
#    folds the literal into the static C decl (compile-time), so the
#    cvar enters the program with the source's value rather than the
#    type default. Conditional rewrites at class body level (e.g.
#    `if X; @@y = ...; end`) are NOT supported -- Spinel doesn't run
#    class-body statements at startup, only constant assignments and
#    method definitions get hoisted.
class Config
  @@debug = true
  @@max_retries = 5

  def self.debug?; @@debug; end
  def self.max_retries; @@max_retries; end
end

puts Config.debug?           # true (folded into static decl)
puts Config.max_retries      # 5


# 4. Cvar updated across many class-method calls -- the cumulative
#    state lives in the cvar slot, not in any temporary. Chained
#    `.add(n).add(m)` returning self isn't supported in Spinel
#    (class methods are static C functions with no self), so we
#    call separately.
class Tally
  @@total = 0

  def self.add(n)
    @@total = @@total + n
  end

  def self.total
    @@total
  end
end

Tally.add(10)
Tally.add(20)
Tally.add(12)
puts Tally.total             # 42


# 5. Cvar mutation across instance and class methods of the same
#    class. Real Ruby code mixes these freely (e.g. ActiveRecord
#    counter caches).
class Registry
  @@items = 0

  def self.count
    @@items
  end

  def register
    @@items = @@items + 1
  end
end

r = Registry.new
r.register
r.register
puts Registry.count          # 2
