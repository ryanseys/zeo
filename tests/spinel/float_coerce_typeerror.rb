# Float arithmetic coerces its operand, and a value with no coercion is a
# TypeError -- the rule Integer already enforced. The Float side had no guard
# at all: `1.5 + nil` evaluated to nil and `1.5 + "s"` emitted double + char *.
p((1.5 + nil rescue $!.message))
p((1.5 + "s" rescue $!.message))
p((1.5 + :a rescue $!.message))
p((1.5 + true rescue $!.message))
p((1.5 + [1] rescue $!.message))
p((1.5 - nil rescue $!.class))
p((1.5 * nil rescue $!.class))
p((1.5 / nil rescue $!.class))
p(1.5 + 1)
p(1.5 + 2.5)
p((1 + nil rescue $!.message))
