# A self-referential array kills the process with a native stack overflow where
# ruby raises `ArgumentError: tried to flatten recursive array`.
#
# `builtins::array::flatten_to_depth` recurses per nested Array with no record
# of what it has already entered, so `a << a` recurses until the Rust stack is
# gone. Ruby tracks the arrays currently being flattened and raises on re-entry
# (`rb_ary_flatten`, using its recursion guard).
#
# `inspect` and `==` already handle the same cycle -- both answer correctly
# below -- so the guard exists elsewhere in the runtime and `flatten` simply
# does not use it. An abort is the worst failure mode available: nothing can
# rescue it, and the exit status is a signal rather than 1.

a = [1]
a << a
p a.inspect
p a == a.dup

begin
  a.flatten
rescue ArgumentError => e
  puts "#{e.class}: #{e.message}"
end

# The depth-limited form stops before re-entry and must NOT raise.
p a.flatten(1).size
