# `Class.new { include M }` mixes M's instance methods into the runtime
# class's ancestry, so instances dispatch them (and reflection sees M).
# Last-included wins on a name collision, matching CRuby's ancestry.

module Greet
  def hello = "hi #{name}"
end
module A; def who = "A"; end
module B; def who = "B"; end
c = Class.new do
  include Greet
  include A
  include B
  def name = "x"
end
obj = c.new
puts obj.hello
puts obj.who
puts c.ancestors.include?(Greet)
puts c.include?(A)
__END__
hi x
B
true
true
