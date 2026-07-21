# An instance_eval/exec splice as the EXPRESSION of a rescue modifier (#2723):
# the expression arm's own preludes must land inside the protected region, and
# the splice temp's seeded type must survive the write pass's per-iteration
# reset (users of the temp precede its synthesized write in node order).
r = ("abc".instance_eval { upcase } rescue $!.class)
p r
r2 = ("ab".instance_exec(3) { |n| self * n } rescue $!.class)
p r2
r3 = (5.instance_eval { self + 1 } rescue :nope)
p r3
# a raise out of the spliced body reaches the enclosing handler with its
# message: the splice rides a ParenthesesNode, not a BeginNode, whose own
# exception region would swallow it
r4 = ("x".instance_eval { raise "boom" } rescue :rescued)
p r4
r5 = ("x".instance_eval { raise "boom" } rescue $!.message)
p r5
