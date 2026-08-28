# `Object#extend` and `def obj.m` change the object from that STATEMENT, not
# from its construction. spinel synthesizes a subclass for the binding and gave
# the object that subclass's identity in its constructor, so a call placed
# before the extend already ran the override and `is_a?` already answered true.
#
# The object now carries its PARENT's cls_id from construction; the statement
# that creates the singleton flips it, and the subclass's own methods check it
# in their prologue -- delegating to the parent's method, or raising CRuby's
# NoMethodError when the parent has none. is_a? reads the id the object
# actually carries instead of folding the static one.

module Bracketed
  def render(text) = "[" + super + "]"
end

class Plain
  def render(text) = text
end

loner = Plain.new
puts "before extend: #{loner.render("hi").inspect}"
puts "before is_a?:  #{loner.is_a?(Bracketed)}"
puts "before class:  #{loner.class}"
loner.extend(Bracketed)
puts "after extend:  #{loner.render("hi").inspect}"
puts "after is_a?:   #{loner.is_a?(Bracketed)}"
puts "after class:   #{loner.class}"

# a sibling instance is untouched, before and after
puts "sibling:       #{Plain.new.render("hi").inspect}"
puts "sibling is_a?: #{Plain.new.is_a?(Bracketed)}"

# def obj.m
a = Plain.new
puts "before def: #{a.render("hi")}"
def a.render(text) = "<" + text + ">"
puts "after def:  #{a.render("hi")}"

# class << obj
b = Plain.new
puts "before scls: #{b.render("hi")}"
class << b
  def render(text) = "{" + text + "}"
end
puts "after scls:  #{b.render("hi")}"

# define_singleton_method
c = Plain.new
puts "before dsm: #{c.render("hi")}"
c.define_singleton_method(:render) { |text| "(" + text + ")" }
puts "after dsm:  #{c.render("hi")}"

# a singleton method the parent does NOT have: before the def it is a
# NoMethodError, which is the arm with nothing to delegate to
class Bare
  def shown = "plain"
end

def try
  yield
rescue NoMethodError
  "NoMethodError"
end

d = Bare.new
puts "before new-name: #{try { d.extra }}"
def d.extra = "added"
puts "after new-name:  #{d.extra}"
puts "still shown:     #{d.shown}"

# the extended object flowing on: the override applies wherever it goes, but
# only after the statement ran
module Loud
  def speak = "!" + super + "!"
end

class Quiet
  def speak = "sh"
end

def say(o) = o.speak

q = Quiet.new
puts "before: #{say(q)}"
q.extend(Loud)
puts "after:  #{say(q)}"

# Two modules on one binding. Each extended module gets its own link in the
# binding's singleton chain, so they stack the way CRuby's ancestry does: a
# later extend sits NEARER the object than an earlier one, and within a single
# `extend(A, B)` the first argument ends up nearest.
module A1
  def tag = "a" + super
end
module B1
  def tag = "b" + super
end
class Base1
  def tag = "-"
end

m = Base1.new
puts "chain none: #{m.tag}"
m.extend(A1)
puts "chain A1:   #{m.tag}"
m.extend(B1)
puts "chain B1:   #{m.tag}"
puts "chain is_a? A1: #{m.is_a?(A1)}"
puts "chain is_a? B1: #{m.is_a?(B1)}"
puts "chain class:    #{m.class}"

# one call, two modules: the first argument ends up nearest
n = Base1.new
n.extend(A1, B1)
puts "both:       #{n.tag}"
puts "both is_a?: #{n.is_a?(A1)} #{n.is_a?(B1)}"

# a sibling with only one of them
o = Base1.new
o.extend(B1)
puts "one only:   #{o.tag}"
puts "one is_a?:  #{o.is_a?(A1)} #{o.is_a?(B1)}"

# a CONSTANT binding takes the same three forms, and its dsm statement is the
# one that emitted nothing at all (a class constant resolves to a class-method
# scope and this node type is shared with it)
SPK = Quiet.new
puts "const before: #{SPK.speak}"
puts "const is_a?:  #{SPK.is_a?(Loud)}"
SPK.extend(Loud)
puts "const after:  #{SPK.speak}"
puts "const is_a?:  #{SPK.is_a?(Loud)}"

CFGD = Quiet.new
puts "cdef before: #{CFGD.speak}"
def CFGD.speak = "def"
puts "cdef after:  #{CFGD.speak}"

CDSM = Quiet.new
puts "cdsm before: #{CDSM.speak}"
CDSM.define_singleton_method(:speak) { "dsm" }
puts "cdsm after:  #{CDSM.speak}"
