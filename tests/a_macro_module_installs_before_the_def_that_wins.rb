module Macro
  def self.included(base) = base.send(:attr_accessor, :m)
end
class C1
  include Macro
  def m = "def wins"
end
p C1.new.m

module Macro2
  def self.extended(base) = base.attr_accessor(:n)
end
class C2
  extend Macro2
  def n = "def wins"
end
p C2.new.n

module Macro3
  def field(x) = attr_accessor(x)
end
class C3
  extend Macro3
  field :o
  def o = "def wins"
end
p C3.new.o

class C4
  instance_eval { attr_accessor :q }
  def q = "def wins"
end
p C4.new.q

class C5
  [:r].each { |x| define_method(x) { "dm" } }
  def r = "def wins"
end
p C5.new.r

class C6
  [:s].each { |x| attr_accessor x }
end
class C6
  def s = "def wins"
end
p C6.new.s

class C7
  def u = "def"
end
class C7
  [:u].each { |x| attr_accessor x }
end
c = C7.new
c.u = 5
p c.u

class C8
  attr_accessor :v
  def v = "def wins"
end
p C8.new.v
