# Instance variables read and write without locking while one Ruby thread
# exists. The moment a second one is spawned that has to stop, for objects
# built BEFORE the spawn as much as after -- so this hammers the same objects
# from both sides of the handoff.

class Counter
  attr_accessor :a, :b, :c

  def initialize
    @a = 0
    @b = 0
    @c = 0
  end

  def bump(n)
    @a += n
    @b = @a * 2
    @c = @b - @a
    self
  end

  def total = @a + @b + @c
end

# Built and driven while this is the only thread.
before = Counter.new
2000.times { |i| before.bump(1) }
p before.total
p before.instance_variables

shared = Array.new(4) { Counter.new }
lock = Mutex.new

threads = 4.times.map do |t|
  Thread.new do
    mine = Counter.new
    500.times { mine.bump(1) }
    lock.synchronize { shared[t].bump(mine.a) }
    mine.total
  end
end
p threads.map(&:value).sort

# The pre-spawn object is still correct, and still writable from the main
# thread now that the fast path is off.
before.bump(1)
p before.total
p shared.map(&:a)

# A thread that only READS an object the main thread built.
seen = Thread.new { before.total }.value
p seen == before.total

# Ivars invented at runtime interleave correctly across the handoff too.
before.instance_variable_set(:@late, 7)
p before.instance_variables
p Thread.new { before.instance_variable_get(:@late) }.value

# Objects created INSIDE a thread never took the fast path at all.
p Thread.new { Counter.new.bump(3).total }.value

# A CLASS-level `@x` is per-class-object storage on the same fast path, and
# it is genuinely shared across threads once one is spawned.
class Registry
  @count = 0
  @names = []

  class << self
    attr_reader :count, :names

    def record(name)
      @count += 1
      @names << name
      @count
    end
  end
end

100.times { |i| Registry.record("pre#{i}") }
p Registry.count

guard = Mutex.new
4.times.map do |t|
  Thread.new { 25.times { |i| guard.synchronize { Registry.record("t#{t}-#{i}") } } }
end.each(&:join)

p Registry.count
p Registry.names.length
p Registry.names.first, Registry.names.last.start_with?("t")
p Thread.new { Registry.count }.value
__END__
8000
[:@a, :@b, :@c]
[2000, 2000, 2000, 2000]
8004
[500, 500, 500, 500]
true
[:@a, :@b, :@c, :@late]
7
12
100
200
200
"pre0"
true
200
