# The small classes zeo lacked: `GC::Profiler`, `Thread::Backtrace` (which is
# what makes the `Location` it namespaces reachable at all), the sibling
# `ObjectSpace::WeakKeyMap`, the `Random::Base`/`Random::Formatter` rungs
# under `Random`, `Enumerator::Generator`/`Producer`, and the `Ractor` error
# tree -- classes only, since zeo runs no ractors.

def show(label)
  puts "#{label}: #{yield.inspect}"
rescue Exception => e
  puts "#{label}: #{e.class}: #{e.message}"
end
# GC::Profiler
show("Profiler singleton") { GC::Profiler.singleton_methods(false).sort }
show("Profiler enabled?") { GC::Profiler.enabled? }
show("Profiler total_time") { GC::Profiler.total_time }
show("Profiler result") { GC::Profiler.result }
show("Profiler raw_data") { GC::Profiler.raw_data }
show("Profiler clear") { GC::Profiler.clear }
show("Profiler enable") { GC::Profiler.enable }
show("Profiler enabled? after") { GC::Profiler.enabled? }
show("Profiler disable") { GC::Profiler.disable }
show("Profiler report") { GC::Profiler.report }
# Thread::Backtrace
show("Backtrace.limit") { Thread::Backtrace.limit }
show("Backtrace singleton") { Thread::Backtrace.singleton_methods(false) }
show("Backtrace constants") { Thread::Backtrace.constants }
show("Location own") { Thread::Backtrace::Location.instance_methods(false).sort }
show("caller_locations class") { caller_locations(0, 1).first.class }
# WeakKeyMap
show("WKM own") { ObjectSpace::WeakKeyMap.instance_methods(false).sort }
show("WKM arities") { ObjectSpace::WeakKeyMap.instance_methods(false).sort.map { |n| ObjectSpace::WeakKeyMap.instance_method(n).arity } }
m = ObjectSpace::WeakKeyMap.new
k = "a"
show("WKM set") { m[k] = 1 }
show("WKM get eq") { m["a".dup] }
show("WKM getkey") { m.getkey("a".dup).equal?(k) }
show("WKM key?") { m.key?("a".dup) }
show("WKM getkey missing") { m.getkey("zz") }
show("WKM inspect") { m.inspect.sub(/0x\h+/,"0xADDR") }
show("WKM int key") { m[1] = 1 }
show("WKM sym key") { m[:s] = 1 }
show("WKM nil key") { m[nil] = 1 }
show("WKM float key") { m[1.5] = 1 }
show("WKM bignum key") { m[2**70] = 1 }
show("WKM array key") { m[[1]] = 2 }
show("WKM delete") { m.delete(k) }
show("WKM delete missing") { m.delete("zz") }
show("WKM delete block") { m.delete("zz") { |x| [:miss, x] } }
show("WKM clear") { m.clear.class }
show("WKM after clear") { m["a"] }
show("WKM new arity") { ObjectSpace::WeakKeyMap.new(1) }
show("WKM frozen write") { f = ObjectSpace::WeakKeyMap.new; f.freeze; f["x"] = 1 }
# Random
show("Random.superclass") { Random.superclass }
show("Random ancestors") { Random.ancestors.firsshow(5) }
show("Random own") { Random.instance_methods(false).sort }
show("Random::Base own") { Random::Base.instance_methods(false).sort }
show("Random::Formatter own has rand") { Random::Formatter.instance_methods(false).include?(:rand) }
show("rand owner") { Random.instance_method(:rand).owner }
show("random_number owner") { Random.instance_method(:random_number).owner }
show("Random.new(42).rand class") { Random.new(42).rand.class }
show("Random.new(42).seed") { Random.new(42).seed }
show("Random.new(42).bytes") { Random.new(42).bytes(4).bytesize }
show("Random.new(42).random_number(10) class") { Random.new(42).random_number(10).class }
# Ractor errors
[Ractor::Error, Ractor::ClosedError, Ractor::IsolationError, Ractor::MovedError, Ractor::RemoteError, Ractor::UnsafeError].each do |c|
  show("#{c} ancestry") { c.ancestors.take_while { |a| a != Object }.map(&:to_s) }
end
show("rescue Ractor::ClosedError") { begin; raise Ractor::ClosedError, "x"; rescue Ractor::ClosedError => e; [e.class, e.message]; end }
show("Ractor::Error is StandardError") { Ractor::Error.new("y").is_a?(StandardError) }
show("Ractor constants") { (Ractor.constants & [:Error, :ClosedError, :IsolationError, :MovedError, :RemoteError, :UnsafeError]).sort }

# `Enumerator::Generator` and `Enumerator::Producer` are the SOURCE objects
# an enumerator holds, not enumerators: a generator answers `#each` and what
# `Enumerable` derives from it, and nothing more.
show("Generator to_a") { Enumerator::Generator.new { |y| y << 1; y << 2 }.to_a }
show("Generator map") { Enumerator::Generator.new { |y| y << 1; y << 2 }.map { |x| x * 3 } }
show("Generator ancestors") { Enumerator::Generator.ancestors.first(3) }
show("Generator own") { Enumerator::Generator.instance_methods(false) }
show("Generator each arity") { Enumerator::Generator.instance_method(:each).arity }
show("Generator inspect") { Enumerator::Generator.new {}.inspect.sub(/0x\h+/, "0xADDR") }
show("Generator no block") { Enumerator::Generator.new }
show("Producer own") { Enumerator::Producer.instance_methods(false) }
show("Producer each arity") { Enumerator::Producer.instance_method(:each).arity }
show("Enumerator.new inspect") { Enumerator.new { |y| y << 1 }.inspect.sub(/0x\h+/, "0xADDR") }
show("Enumerator.new to_a") { Enumerator.new { |y| y << 1; y << 2 }.to_a }
show("Enumerator.new next") do
  e = Enumerator.new { |y| y << 1; y << 2 }
  [e.next, e.next]
end
show("Enumerator.new size") { Enumerator.new(3) { |y| y << 1 }.size }
show("Enumerator.new size proc") { Enumerator.new(-> { 7 }) { |y| y << 1 }.size }
show("produce class") { Enumerator.produce(1) { |x| x + 1 }.class }
show("produce first") { Enumerator.produce(1) { |x| x + 1 }.first(3) }
show("produce size") { Enumerator.produce(1) { |x| x + 1 }.size }
show("produce inspect") { Enumerator.produce(1) { |x| x + 1 }.inspect.sub(/0x\h+/, "0xADDR") }
show("Enumerator constants") { Enumerator.constants.sort }
__END__
Profiler singleton: [:clear, :disable, :enable, :enabled?, :raw_data, :report, :result, :total_time]
Profiler enabled?: false
Profiler total_time: 0.0
Profiler result: ""
Profiler raw_data: nil
Profiler clear: nil
Profiler enable: nil
Profiler enabled? after: true
Profiler disable: nil
Profiler report: nil
Backtrace.limit: -1
Backtrace singleton: [:limit]
Backtrace constants: [:Location]
Location own: [:absolute_path, :base_label, :inspect, :label, :lineno, :path, :to_s]
caller_locations class: Thread::Backtrace::Location
WKM own: [:[], :[]=, :clear, :delete, :getkey, :inspect, :key?]
WKM arities: [1, 2, 0, 1, 1, 0, 1]
WKM set: 1
WKM get eq: 1
WKM getkey: true
WKM key?: true
WKM getkey missing: nil
WKM inspect: "#<ObjectSpace::WeakKeyMap:0xADDR size=1>"
WKM int key: ArgumentError: WeakKeyMap keys must be garbage collectable
WKM sym key: ArgumentError: WeakKeyMap keys must be garbage collectable
WKM nil key: ArgumentError: WeakKeyMap keys must be garbage collectable
WKM float key: ArgumentError: WeakKeyMap keys must be garbage collectable
WKM bignum key: ArgumentError: WeakKeyMap keys must be garbage collectable
WKM array key: 2
WKM delete: 1
WKM delete missing: nil
WKM delete block: [:miss, "zz"]
WKM clear: ObjectSpace::WeakKeyMap
WKM after clear: nil
WKM new arity: ArgumentError: wrong number of arguments (given 1, expected 0)
WKM frozen write: 1
Random.superclass: Random::Base
Random ancestors: NoMethodError: undefined method 'firsshow' for an instance of Array
Random own: [:==]
Random::Base own: [:bytes, :rand, :seed]
Random::Formatter own has rand: true
rand owner: Random::Base
random_number owner: Random::Formatter
Random.new(42).rand class: Float
Random.new(42).seed: 42
Random.new(42).bytes: 4
Random.new(42).random_number(10) class: Integer
Ractor::Error ancestry: ["Ractor::Error", "RuntimeError", "StandardError", "Exception"]
Ractor::ClosedError ancestry: ["Ractor::ClosedError", "StopIteration", "IndexError", "StandardError", "Exception"]
Ractor::IsolationError ancestry: ["Ractor::IsolationError", "Ractor::Error", "RuntimeError", "StandardError", "Exception"]
Ractor::MovedError ancestry: ["Ractor::MovedError", "Ractor::Error", "RuntimeError", "StandardError", "Exception"]
Ractor::RemoteError ancestry: ["Ractor::RemoteError", "Ractor::Error", "RuntimeError", "StandardError", "Exception"]
Ractor::UnsafeError ancestry: ["Ractor::UnsafeError", "Ractor::Error", "RuntimeError", "StandardError", "Exception"]
rescue Ractor::ClosedError: [Ractor::ClosedError, "x"]
Ractor::Error is StandardError: true
Ractor constants: [:ClosedError, :Error, :IsolationError, :MovedError, :RemoteError, :UnsafeError]
Generator to_a: [1, 2]
Generator map: [3, 6]
Generator ancestors: [Enumerator::Generator, Enumerable, Object]
Generator own: [:each]
Generator each arity: -1
Generator inspect: "#<Enumerator::Generator:0xADDR>"
Generator no block: LocalJumpError: no block given
Producer own: [:each]
Producer each arity: 0
Enumerator.new inspect: "#<Enumerator: #<Enumerator::Generator:0xADDR>:each>"
Enumerator.new to_a: [1, 2]
Enumerator.new next: [1, 2]
Enumerator.new size: 3
Enumerator.new size proc: 7
produce class: Enumerator
produce first: [1, 2, 3]
produce size: Infinity
produce inspect: "#<Enumerator: #<Enumerator::Producer:0xADDR>:each>"
Enumerator constants: [:ArithmeticSequence, :Chain, :Generator, :Lazy, :Producer, :Product, :Yielder]
