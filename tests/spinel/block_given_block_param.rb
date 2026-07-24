# `block_given?` for `&block` parameters.
#
# Pre-fix `block_given?` only returned the right answer inside methods
# that used `yield` (`@in_yield_method == 1`). Methods that captured
# the block solely via `&block` and never invoked yield fell through
# to the constant `"0"` — losing the answer for the one case where
# the runtime *does* know whether a block was forwarded.
#
# Fix: track the enclosing method's `&block` param name in
# `@current_method_block_param`, set/restored at all three method-emit
# sites (emit_instance_method, emit_class_level_method,
# emit_toplevel_method) via a new find_block_param_name helper. The
# block_given? handler returns `(lv_<param> != NULL)` after the
# existing `@in_yield_method` branch.
#
# This test exercises the no-block-passed observable through the
# receiverless default-padding path and the explicit `self.block_given?`
# spelling.

# 1. Top-level `&block` method, called without a block — block_given?
#    must return false. Pre-fix this constant-returned 0 with no
#    visibility into the actual presence of a block.
def top(&block)
  if block_given?
    puts "1-yes"
  else
    puts "1-no"
  end
end

top                       #=> 1-no

# 2. Same shape with a regular arg before &block — find_block_param_name
#    must skip the int param and pick the trailing proc-typed slot.
def top2(label, &block)
  if block_given?
    puts label + "-yes"
  else
    puts label + "-no"
  end
end

top2("2")                 #=> 2-no

def top3(&block)
  if self.block_given?
    puts "3-yes"
  else
    puts "3-no"
  end
end

def top4(&block)
  if self.block_given?
    puts "4-yes"
  else
    puts "4-no"
  end
end

top3                      #=> 3-no
top4 {}                   #=> 4-yes

puts "done"
