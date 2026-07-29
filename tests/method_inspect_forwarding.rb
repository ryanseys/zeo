# The anonymous forwarding parameters (`*`, `**`, `&`, and the `...` that means
# all three) in `#parameters` and in `#inspect`'s signature. Ruby names each
# anonymous slot for its own sigil, and renders the group with rules that are
# not simply "print what was written" -- every combination is pinned here.
def s(m) = m.inspect.sub(/ [^ ]+:\d+>\z/, ">")

class S
  def a(*, **, &) = 1
  def b(x, *, **, &) = 1
  def c(*) = 1
  def d(**) = 1
  def e(&) = 1
  def f(*, **) = 1
  def g(*, &) = 1
  def h(**, &) = 1
  def i(...) = 1
  def j(x, ...) = 1
  def k(*r, **kw, &blk) = 1
  def l(x, *, y) = 1
end
%i[a b c d e f g h i j k l].each { |n| puts "#{n}: #{s(S.instance_method(n))}" }
%i[a b c d e f g h i j k l].each { |n| puts "#{n}: #{S.instance_method(n).parameters.inspect}" }

# `...` forwards positionals, keywords and the block -- through a def the
# compiler saw, and through one an eval built at runtime.
class T
  def target(a, b = 2, *r, k: 3, &blk) = [a, b, r, k, blk&.call]
  def fwd(...) = target(...)
  eval <<~RUBY
    def fwd_eval(...)
      target(...)
    end
  RUBY
end
p T.new.fwd(1)
p T.new.fwd(1, 9, 8, k: 7) { :blk }
p T.new.fwd_eval(1)
p T.new.fwd_eval(1, 9, 8, k: 7) { :blk }
