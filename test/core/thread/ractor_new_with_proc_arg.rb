# `Ractor.new(&proc)` -- a dynamic proc converts like any `&` block
# argument, and its isolability is decided at Ractor.new TIME with CRuby's
# plain ArgumentError (only a literal block gets zeo's earlier static
# check). The proc carries its capture verdict from its creation site.
pred = proc { |x| x * 2 }
puts Ractor.new(21, &pred).value

# The forwarded-block shapes (grepfruit's `Ractor.new(port, &)`).
def spawn_worker(&blk)
  Ractor.new(10, &blk)
end
puts spawn_worker { |x| x + 5 }.value

def create_worker(&)
  Ractor.new(3, &)
end
puts create_worker { |x| x - 1 }.value

# The ivar-held shape (pbt's `Ractor.new(val, &@predicate)`).
class Runner
  def initialize(&blk)
    @predicate = blk
  end

  def run(val)
    Ractor.new(val, &@predicate)
  end
end
puts Runner.new { |v| v * v }.run(7).value

# A lambda converts the same way.
doubler = ->(x) { x + x }
puts Ractor.new(4, &doubler).value

# A proc that captures an outer variable refuses at Ractor.new time,
# catchably -- never at the proc's creation.
outer = 1
bad = proc { outer + 1 }
begin
  Ractor.new(&bad)
rescue ArgumentError => e
  puts e.message
end

# `&nil` is "no block" -- the missing-block ArgumentError.
begin
  Ractor.new(&nil)
rescue ArgumentError => e
  puts e.message
end
__END__
42
15
2
49
8
can not isolate a Proc because it accesses outer variables (outer).
must be called with a block
#@ stderr
core/thread/ractor_new_with_proc_arg.rb:6: warning: Ractor API is experimental and may change in future versions of Ruby.
