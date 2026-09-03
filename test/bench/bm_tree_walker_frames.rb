# The other half of bm_tree_walker's shape: twelve node classes, a
# String-keyed environment, and a fresh env frame allocated per interpreted
# call -- so it pays for per-call ALLOCATION and hash writes, not only for
# the visit dispatch. Interpreted fib, which is all call frames.
class Num;  attr_reader :v;        def initialize(v); @v = v; end; end
class Str;  attr_reader :s;        def initialize(s); @s = s; end; end
class Var;  attr_reader :name;     def initialize(n); @name = n; end; end
class Asgn; attr_reader :name, :e; def initialize(n, e); @name = n; @e = e; end; end
class Add;  attr_reader :l, :r;    def initialize(l, r); @l = l; @r = r; end; end
class Sub;  attr_reader :l, :r;    def initialize(l, r); @l = l; @r = r; end; end
class Mul;  attr_reader :l, :r;    def initialize(l, r); @l = l; @r = r; end; end
class Lt;   attr_reader :l, :r;    def initialize(l, r); @l = l; @r = r; end; end
class If;   attr_reader :c, :t, :f; def initialize(c, t, f); @c = c; @t = t; @f = f; end; end
class Seq;  attr_reader :xs;       def initialize(xs); @xs = xs; end; end
class Call; attr_reader :name, :args; def initialize(n, a); @name = n; @args = a; end; end
class Fn;   attr_reader :params, :body; def initialize(p, b); @params = p; @body = b; end; end

class Interp
  def initialize
    @fns = {}
  end

  def define(name, fn)
    @fns[name] = fn
  end

  def visit(node, env)
    if node.is_a?(Num)
      node.v
    elsif node.is_a?(Str)
      node.s
    elsif node.is_a?(Var)
      env[node.name]
    elsif node.is_a?(Asgn)
      env[node.name] = visit(node.e, env)
    elsif node.is_a?(Add)
      visit(node.l, env) + visit(node.r, env)
    elsif node.is_a?(Sub)
      visit(node.l, env) - visit(node.r, env)
    elsif node.is_a?(Mul)
      visit(node.l, env) * visit(node.r, env)
    elsif node.is_a?(Lt)
      visit(node.l, env) < visit(node.r, env)
    elsif node.is_a?(If)
      visit(node.c, env) ? visit(node.t, env) : visit(node.f, env)
    elsif node.is_a?(Seq)
      r = 0
      node.xs.each { |x| r = visit(x, env) }
      r
    elsif node.is_a?(Call)
      fn = @fns[node.name]
      frame = {}
      i = 0
      fn.params.each do |p|
        frame[p] = visit(node.args[i], env)
        i += 1
      end
      visit(fn.body, frame)
    else
      0
    end
  end
end

ip = Interp.new
# fib(n) = n < 2 ? n : fib(n-1) + fib(n-2)
ip.define("fib", Fn.new(["n"],
  If.new(Lt.new(Var.new("n"), Num.new(2)),
         Var.new("n"),
         Add.new(Call.new("fib", [Sub.new(Var.new("n"), Num.new(1))]),
                 Call.new("fib", [Sub.new(Var.new("n"), Num.new(2))])))))
prog = Call.new("fib", [Num.new(24)])
p ip.visit(prog, {})
__END__
46368
