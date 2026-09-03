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

puts "sibling:       #{Plain.new.render("hi").inspect}"
puts "sibling is_a?: #{Plain.new.is_a?(Bracketed)}"

a = Plain.new
puts "before def: #{a.render("hi")}"
def a.render(text) = "<" + text + ">"
puts "after def:  #{a.render("hi")}"

b = Plain.new
puts "before scls: #{b.render("hi")}"
class << b
  def render(text) = "{" + text + "}"
end
puts "after scls:  #{b.render("hi")}"

c = Plain.new
puts "before dsm: #{c.render("hi")}"
c.define_singleton_method(:render) { |text| "(" + text + ")" }
puts "after dsm:  #{c.render("hi")}"

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

n = Base1.new
n.extend(A1, B1)
puts "both:       #{n.tag}"
puts "both is_a?: #{n.is_a?(A1)} #{n.is_a?(B1)}"

o = Base1.new
o.extend(B1)
puts "one only:   #{o.tag}"
puts "one is_a?:  #{o.is_a?(A1)} #{o.is_a?(B1)}"

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
__END__
before extend: "hi"
before is_a?:  false
before class:  Plain
after extend:  "[hi]"
after is_a?:   true
after class:   Plain
sibling:       "hi"
sibling is_a?: false
before def: hi
after def:  <hi>
before scls: hi
after scls:  {hi}
before dsm: hi
after dsm:  (hi)
before new-name: NoMethodError
after new-name:  added
still shown:     plain
before: sh
after:  !sh!
chain none: -
chain A1:   a-
chain B1:   ba-
chain is_a? A1: true
chain is_a? B1: true
chain class:    Base1
both:       ab-
both is_a?: true true
one only:   b-
one is_a?:  false true
const before: sh
const is_a?:  false
const after:  !sh!
const is_a?:  true
cdef before: sh
cdef after:  def
cdsm before: sh
cdsm after:  dsm
