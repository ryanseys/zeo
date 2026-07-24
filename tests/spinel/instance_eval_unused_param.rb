# instance_eval/instance_exec with an unused block parameter must still
# compile: the param gets no C declaration, so its binding is skipped (the
# exec arg still evaluates for effect).
p(true.instance_eval { |o| self })
p(5.instance_eval { |o| self * 2 })
p("ab".instance_exec(3) { |n| self })
p(7.instance_eval { |o| o + 1 })
class W; def initialize; @v = 9; end; def go; instance_eval { |w| @v }; end; end
p W.new.go
class U; def initialize; @z = 4; end; def zz; @z; end; end
u = U.new
p(u.instance_eval { |o| self.zz })
p(u.instance_exec(5) { |n| zz })
p(u.instance_exec(5) { |n| n + zz })
side = []
p(1.instance_exec(side << :evaluated) { |a| self })
p side
p(true.instance_exec(1, 2) { |a, b| self })
