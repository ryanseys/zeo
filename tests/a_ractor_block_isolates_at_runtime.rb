# `Ractor.new { ... }` -- isolation is CRuby's runtime verdict, not zeo's
# compile-time one.
#
# zeo used to refuse to compile a block that touched an outer local or the
# enclosing object's ivars. Ruby compiles both, and answers differently for
# each:
#
#   - an OUTER LOCAL is an `ArgumentError` from `Ractor.new` itself, raised
#     after the arguments have been evaluated;
#   - an `@ivar` is not a Proc problem at all. Ruby isolates the Proc and
#     rebinds its `self` to the RACTOR, so the read is an ivar of a shareable
#     object and raises `Ractor::IsolationError` INSIDE the ractor, surfacing
#     as `Ractor::RemoteError` at `#value`;
#   - a receiverless call to an enclosing-class method is a plain `NameError`
#     for the same reason: `self` is a Ractor, which has no such method.
Warning[:experimental] = false
# A ractor that aborts also prints `#<Thread:0x...> terminated with exception`
# on stderr, and the address makes that unpinnable in a golden. Silenced here
# so the test compares the part that is deterministic; zeo does not print the
# report at all, which is a separate (recorded) divergence.
Thread.report_on_exception = false

def arg = (puts "the argument still ran"; 1)

outer = 5
begin
  Ractor.new(arg) { outer }
rescue => e
  p e.class
  p e.message
end

class Holder
  def initialize
    @n = 7
  end

  def cause_of
    yield.value
    "no raise"
  rescue Ractor::RemoteError => e
    [e.cause.class.to_s, e.cause.message]
  end

  def report
    p cause_of { Ractor.new { @n } }
    p cause_of { Ractor.new { @n = 3 } }
    p cause_of { Ractor.new { defined?(@n) } }
    p cause_of { Ractor.new { instance_variable_get(:@n) } }
    p cause_of { Ractor.new { instance_variable_set(:@n, 1) } }
    p cause_of { Ractor.new { helper } }
  end

  def helper = "helped"
end

Holder.new.report

# `self` inside the block, and the two reflective reads ruby permits there.
r = Ractor.new { [self.class.to_s, Ractor.shareable?(self), instance_variables] }
p r.value

# A block that touches neither still runs, and its argument crosses.
p Ractor.new(3) { |n| n * 2 }.value

# The MAIN ractor is untouched: an ordinary ivar read of a shareable receiver
# is only an error from a non-main ractor.
p Holder.new.instance_variable_get(:@n)
FROZEN = Object.new.freeze
p FROZEN.instance_variables
