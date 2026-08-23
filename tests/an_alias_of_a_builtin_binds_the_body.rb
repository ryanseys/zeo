# An alias of a builtin binds the BODY it names, not the name. A later `def`
# of the source must not capture it -- that made the standard wrap idiom
# recurse. Eleven shapes: the two spellings, no redefinition, a class-method
# alias, a subclass receiver (the payload bridge), a module method, reflection,
# an alias of an alias, and the two indirect call forms.

class String
  alias_method :s_old, :upcase
  def upcase = "w(#{s_old})"
end
class Array
  alias a_old first
  def first(*) = "replaced"
end
class Hash
  alias_method :h_old, :size
end
class Foo
  class << self
    alias mk new
  end
end
class MyStr < String
  alias_method :m_old, :length
  def length = 99
end
module Mixin
  def mixed = "mixed"
end
class Bar
  include Mixin
  alias_method :b_old, :mixed
  def mixed = "w(#{b_old})"
end
class Float
  alias_method :f1, :round
  alias_method :f2, :f1
end

puts "a: #{'ab'.upcase} / #{'ab'.s_old}"
puts "b: #{[1,2].first.inspect} / #{[1,2].a_old.inspect}"
puts "c: #{({x: 1}).h_old}"
puts "d: #{Foo.mk.class}"
puts "e: #{MyStr.new('hi').length} / #{MyStr.new('hi').m_old}"
puts "f: #{Bar.new.mixed} / #{Bar.new.b_old}"
puts "g: #{'x'.respond_to?(:s_old)} / #{String.method_defined?(:s_old)}"
puts "h: #{String.instance_method(:s_old).arity}"
puts "i: #{1.7.f2}"
puts "j: #{'ab'.method(:s_old).call}"
puts "k: #{'ab'.send(:s_old)}"
