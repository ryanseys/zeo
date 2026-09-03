# The cycle this program builds on purpose, and the census the
# `ZEO_RT_GCCHECK=1` leg gates against. A cycle alive at exit is not a
# defect: it is a ring the program never broke. What the leg gates is a
# CHANGE to the line below.
# 
# A Foo stores a block that captured `self`.
#@ gccheck: cycle leak: 3 objects (Array x1, Foo x1, Proc x1)
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
__END__
hi
