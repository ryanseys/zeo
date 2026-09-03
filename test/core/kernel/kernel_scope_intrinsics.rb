# Four `Kernel` functions cannot be reached through `send`, `method` or
# `respond_to?` -- but every probe here uses a LITERAL symbol, so all of them
# are mechanically fixable: a literal `send(:block_given?)` can fold into the
# caller exactly like the direct spelling (the `is_sent_eval` precedent in
# `codegen::captures`), `respond_to?`/`method` need only a raising row to
# exist. Called DIRECTLY they all work already; the folds just don't
# recognize the reflective spellings.
#
# The genuinely hard shape -- `send(name)` with a name COMPUTED at runtime,
# where no fold can see the caller's block or scope -- is split out into
# `kernel_scope_intrinsics_dynamic_send.rb`.

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
__END__
direct block_given?: true
send block_given?: true
send iterator?: false
send binding: Binding
send local_variables: true
respond_to? block_given?: true
method(:binding): Method
direct binding: Binding
direct local_variables: true
