# `def` inside `eval`/`class_eval`/`instance_eval`
# (installing on the right definee), optional+rest+keyword params, `return`
# and `yield` in an eval-defined method, and blocks + auto-splat passed to
# calls inside eval. Each source is non-literal (held in a variable / built
# with a heredoc) so it runs through the runtime `eval`, not the inline
# splice.

code = <<~RUBY
  def calc(a, b = 10, *rest)
    return a + b if rest.empty?
    a + b + rest.sum
  end
RUBY
eval(code)
puts calc(1)
puts calc(1, 2, 3, 4)
eval("def each_twice; yield 1; yield 2; end")
out = []
each_twice { |n| out << n * 10 }
p out
class Widget; end
Widget.class_eval("def name; \"widget\"; end")
puts Widget.new.name
obj = Object.new
obj.instance_eval("def secret; 42; end")
puts obj.secret
p Object.new.respond_to?(:secret)
eval("def kw(a, x:, y: 5); [a, x, y]; end")
p kw(1, x: 2)
begin; kw(1); rescue ArgumentError => e; puts e.message; end
begin; calc; rescue ArgumentError => e; puts e.message; end
p eval("[1, 2, 3].map { |x| x * 2 }")
p eval("[[1, 2], [3, 4]].map { |a, b| a + b }")
p eval("def foo; end")
__END__
11
10
[10, 20]
widget
42
false
[1, 2, 5]
missing keyword: :x
wrong number of arguments (given 0, expected 1+)
[2, 4, 6]
[3, 7]
:foo
