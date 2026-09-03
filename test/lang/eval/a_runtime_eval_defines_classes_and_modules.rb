# `class`/`module` bodies inside eval: a fresh class with initialize+ivars,
# a subclass whose method calls `super`, a module function, reopening a
# compiled class, and the class expression's value. Non-literal source ->
# the runtime `eval` entry. Oracle-verified verbatim.

code = <<~RUBY
  class Point
    def initialize(x, y)
      @x = x
      @y = y
    end
    def sum
      @x + @y
    end
  end
RUBY
eval(code)
p Point.new(3, 4).sum
eval("class Base; def kind; \"base\"; end; end; class Sub < Base; def kind; \"sub:\" + super; end; end")
p Sub.new.kind
eval("module Helpers; def self.double(n); n * 2; end; end")
p Helpers.double(21)
class Widget
  def base; 1; end
end
eval("class Widget; def extra; base + 10; end; end")
p Widget.new.extra
p eval("class Empty; 99; end")
__END__
7
"sub:base"
42
11
99
