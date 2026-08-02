# The `Kernel` functions the compiler folds straight into the caller, asked
# for the three ways a fold cannot answer: `send`, `method` and `respond_to?`.
#
# `printf(...)` has always worked, because `codegen::call::kernel` compiles it
# in place. Without a method-table row, though, `send(:printf, ...)` raised
# NoMethodError, `method(:printf)` raised NameError, `respond_to?(:printf,
# true)` answered false, and `Kernel.printf(...)` had nowhere to land. Each
# row calls the very function the fold calls, so the two forms cannot
# diverge.

def show(label)
  puts "#{label}: #{yield.inspect}"
rescue Exception => e # rubocop:disable Lint/RescueException
  puts "#{label}: #{e.class}: #{e.message}"
end

NAMES = %i[
  Pathname printf exit exit! abort at_exit lambda fork syscall set_trace_func
  __method__ __callee__ __dir__
].freeze

# `syscall` is a NOTIMPLEMENT STUB on this platform, and ruby answers
# `respond_to?` false for those while still listing them -- see
# `tests/gaps/notimplement_stub_respond_to.rb`.
ASKABLE = NAMES - %i[syscall]

# ruby files all of these as PRIVATE instance methods and PUBLIC singleton
# methods of Kernel -- what `module_function` means.
show("private instance") { NAMES.all? { |m| Kernel.private_instance_methods(false).include?(m) } }
show("public singleton") { NAMES.all? { |m| Kernel.singleton_methods(false).include?(m) } }
show("not public instance") { NAMES.none? { |m| Kernel.instance_methods(false).include?(m) } }
show("respond_to? private") { ASKABLE.all? { |m| respond_to?(m, true) } }
show("respond_to? public") { ASKABLE.none? { |m| respond_to?(m) } }
show("method objects") { NAMES.map { |m| method(m).class }.uniq }
show("arities") { NAMES.to_h { |m| [m, Kernel.instance_method(m).arity] } }

# Each one, called through `send`.
show("send printf") { send(:printf, "%d-%s\n", 1, "a"); :printed }
show("Kernel.printf") { Kernel.printf("%04b\n", 5); :printed }
show("send lambda") { l = send(:lambda) { |x| x * 2 }; [l.class, l.lambda?, l.call(3)] }
show("send Pathname") { send(:Pathname, "/x").to_s }
show("send exit") { begin; send(:exit, 3); rescue SystemExit => e; e.status; end }
show("send exit true") { begin; send(:exit); rescue SystemExit => e; e.status; end }
show("send exit false") { begin; send(:exit, false); rescue SystemExit => e; e.status; end }
show("send abort") { begin; send(:abort, "boom"); rescue SystemExit => e; [e.status, e.message]; end }
show("send syscall") { send(:syscall, 999_999) }
show("send set_trace_func nil") { send(:set_trace_func, nil) }
show("send at_exit") { send(:at_exit) { nil }.class }

# The frame readers. A builtin row pushes no frame of its own, so the current
# frame is the CALLER's -- which is what makes these answer at all.
def named_method = send(:__method__)
def named_callee = send(:__callee__)
class Holder
  def inside = send(:__method__)
end
show("send __method__") { named_method }
show("direct __method__") { def d = __method__; d }
show("send __callee__") { named_callee }
show("__method__ in a class") { Holder.new.inside }
show("__method__ at top level") { send(:__method__) }
show("send __dir__ class") { send(:__dir__).class }
show("__dir__ agrees with File") { send(:__dir__) == File.dirname(File.expand_path(__FILE__)) }
show("Kernel.__dir__ is a String") { Kernel.__dir__.class }

# `fork` is `Process.fork` under another name, so a gem's `Process._fork`
# hook stays in the path.
show("fork runs the child") do
  pid = send(:fork) { exit 7 }
  Process.wait2(pid)[1].exitstatus
end
