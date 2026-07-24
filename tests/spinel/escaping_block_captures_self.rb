# A block that escapes inlining -- stored in an ivar, so it becomes a real
# _proc_<n> function -- and references self (here calls the instance method
# `record` and, through it, the `@log` ivar) must capture self through the proc
# cap struct. Otherwise the proc body emits `self` with no parameter or capture
# for it -> "use of undeclared identifier 'self'". Verified via the side effect
# (record appends to @log), not the block's return value.
class Foo
  def initialize
    @log = [""]
    @log.pop
  end
  def record(x)
    @log.push(x)
  end
  def store(&blk)
    @blk = blk
  end
  def setup
    store { record("hi") }   # block calls instance method `record` -> needs self
  end
  def fire
    @blk.call
  end
  def first
    @log[0]
  end
end

f = Foo.new
f.setup
f.fire
puts f.first
