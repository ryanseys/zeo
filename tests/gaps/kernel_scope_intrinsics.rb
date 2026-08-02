# Four `Kernel` functions cannot be reached through `send`, because a method
# row cannot see what they need.
#
# `block_given?`/`iterator?` need the CALLER's block, and `binding`/
# `local_variables` need the caller's local scope. Neither travels: a
# builtin row receives its own call's block and no scope at all, and zeo's
# `Frame` deliberately carries only `(file, line, label)` -- 40 bytes,
# pushed on every call -- so widening it to carry a block handle and a scope
# pointer would tax every call in the program for a reflection path almost
# nothing takes.
#
# Called DIRECTLY they all work: `codegen::call::kernel` folds each into the
# caller, where the block and the scope are right there. Only the `send`,
# `method` and `respond_to?` forms are missing.

def show(label)
  puts "#{label}: #{yield.inspect}"
rescue Exception => e # rubocop:disable Lint/RescueException
  puts "#{label}: #{e.class}: #{e.message}"
end

def with_block = block_given?
def sent_block = send(:block_given?)

show("direct block_given?") { with_block { } }
show("send block_given?") { sent_block { } }
show("send iterator?") { send(:iterator?) }
show("send binding") { send(:binding).class }
show("send local_variables") { x = 1; send(:local_variables).include?(:x) }
show("respond_to? block_given?") { respond_to?(:block_given?, true) }
show("method(:binding)") { method(:binding).class }

# Direct calls are unaffected, which is what makes this a `send`-shape gap.
show("direct binding") { binding.class }
show("direct local_variables") { y = 2; local_variables.include?(:y) }
