# `TOPLEVEL_BINDING` IS the top-level frame's binding. It is installed
# unconditionally so `Object.constants` lists it, but naming it anywhere in
# the program -- including from inside a required library's method, which is
# how erb reaches it -- is what deoptimizes the top level to cells so the
# binding has locals to report.
#
# NOTE: `Object.const_source_location(:TOPLEVEL_BINDING)` is `["<main>", 0]`
# in ruby and under CLIF, and `[]` under the rustc backend. Not pinned here
# while both backends run this corpus.

top_a = 1
top_b = "two"

p TOPLEVEL_BINDING.class
p Object.constants.include?(:TOPLEVEL_BINDING)
p TOPLEVEL_BINDING.receiver
p TOPLEVEL_BINDING.local_variables.sort
p TOPLEVEL_BINDING.local_variable_get(:top_a)
p TOPLEVEL_BINDING.eval("top_a.to_s + top_b")

# It is main's frame, so a write through it lands there.
TOPLEVEL_BINDING.local_variable_set(:top_a, 42)
p top_a

# ...and reaching it from inside a method still names main's frame, not the
# method's.
def from_a_method
  local_to_method = :hidden
  [TOPLEVEL_BINDING.local_variables.include?(:local_to_method),
   TOPLEVEL_BINDING.local_variables.include?(:top_b)]
end
p from_a_method

# The same object every time.
p TOPLEVEL_BINDING.equal?(TOPLEVEL_BINDING)
