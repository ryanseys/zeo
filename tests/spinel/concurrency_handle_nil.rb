# `nil?` on a slot typed as a concurrency handle answered a compile-time false,
# on the reasoning that a live handle is never NULL. The slot holding one can
# be, though: `@t = nil` then `if @t.nil?` folded to false, so the guarded
# `Thread.new` never ran and every later call went through the NULL still in
# the ivar -- a segfault in Thread#alive? / #kill rather than a thread.
class A
  def initialize
    @t = nil
  end

  def go
    if @t.nil?
      @t = Thread.new { sleep(0.05) }
    end
    puts "thread created"
  end

  attr_reader :t
end

a = A.new
p a.t.nil?
a.go
p a.t.nil?
p a.t.class
p a.t.alive?
a.t.kill
puts "thread ok"

# the guard also has to stay false once it is set, so a second call is a no-op
a.go

# the same slot shape for the other three handles
class B
  def initialize
    @q = nil
    @m = nil
    @c = nil
  end

  def fill
    @q = Queue.new if @q.nil?
    @m = Mutex.new if @m.nil?
    @c = ConditionVariable.new if @c.nil?
  end

  def report
    p [@q.nil?, @m.nil?, @c.nil?]
  end

  attr_reader :q, :m
end

b = B.new
b.report
b.fill
b.report
b.q.push(1)
p b.q.pop
p b.m.locked?

# a local, which takes the same arm
t = nil
p t.nil?
t = Thread.new { 1 }
p t.nil?
t.join
puts "done"
