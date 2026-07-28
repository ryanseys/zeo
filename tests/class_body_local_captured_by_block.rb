# A class body is an ordinary Ruby scope with ordinary locals, and a block
# written in it closes over them -- optparse builds its whole accept table this
# way (`yesno = CompletingHash.new; %w[- no].each { |el| yesno[el] = false }`),
# so rubygems' every option parse depends on it.
class CompletingHash < Hash
  def complete(key) = self[key]
end

class Host
  yesno = CompletingHash.new
  %w[- no false].each { |el| yesno[el] = false }
  %w[+ yes true].each { |el| yesno[el] = true }
  yesno["nil"] = false
  TABLE = yesno

  # Mutated by the block, then read back outside it.
  count = 0
  3.times { count += 1 }
  COUNT = count

  # A local first ASSIGNED inside the block is block-owned, and does not leak.
  [1].each { fresh = :inside }
  LEAKED = defined?(fresh)

  # Nested blocks reach the same binding.
  seen = []
  [[1, 2], [3]].each { |row| row.each { |v| seen << v } }
  SEEN = seen

  # ...and a method defined in the same body does NOT see the class body's
  # locals, matching Ruby's scope rules.
  def self.reads_table = TABLE
end

p Host::TABLE.class
p Host::TABLE.sort.to_h
p Host::TABLE.complete("no")
p Host::COUNT
p Host::LEAKED
p Host::SEEN
p Host.reads_table.size

# The same shape inside a module body.
module Registry
  entries = {}
  %w[a b].each { |k| entries[k] = k.upcase }
  ENTRIES = entries
end
p Registry::ENTRIES

# ...and a block that outlives the body still sees the binding.
class Deferred
  pending = []
  ADD = ->(v) { pending << v }
  READ = -> { pending }
end
Deferred::ADD.call(1)
Deferred::ADD.call(2)
p Deferred::READ.call
