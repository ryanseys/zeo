# Visibility is not only a call-time gate: `respond_to?`, `singleton_methods`
# and the `*_instance_methods` family all report it, and code branches on what
# they say. Three places where zeo enforced the gate but recorded nothing.

puts "-- Module's own directives are private methods of Module"
p Module.private_instance_methods(false).sort &
  %i[private public protected module_function remove_const ruby2_keywords included extended prepended]
p Class.private_instance_methods(false) & [:inherited]

class C; end
%i[private public protected module_function remove_const ruby2_keywords inherited].each do |m|
  begin
    C.public_send(m)
    puts "C.#{m}: answered"
  rescue NoMethodError
    puts "C.#{m}: NoMethodError"
  rescue ArgumentError
    puts "C.#{m}: ArgumentError"
  end
end

puts "-- a singleton class sees them with Module's visibility"
p C.singleton_class.method_defined?(:private)
p C.singleton_class.private_method_defined?(:private)
p C.singleton_class.method_defined?(:name)

puts "-- while the class-body directives still work (implicit self)"
class D
  def a = 1
  private
  def b = 2
  public
  def c = 3
end
p [D.new.a, D.new.c]
begin
  D.new.b
rescue NoMethodError
  puts "D#b is private"
end

puts "-- private inside `class << self`, on a user module"
module UserMod
  class << self
    def pub = 1
    private
    def hid = 2
  end
end
p UserMod.respond_to?(:hid)
p UserMod.singleton_methods(false).sort
p UserMod.singleton_class.private_instance_methods(false).sort
begin
  UserMod.hid
rescue NoMethodError
  puts "UserMod.hid: NoMethodError"
end

puts "-- and on a REOPENED BUILTIN, which recorded nothing at all"
module Comparable
  class << self
    def cmp_pub = 1
    private
    def cmp_hid = 2
  end
end
p Comparable.respond_to?(:cmp_hid)
p Comparable.singleton_class.private_instance_methods(false).include?(:cmp_hid)
p Comparable.cmp_pub
begin
  Comparable.cmp_hid
rescue NoMethodError
  puts "Comparable.cmp_hid: NoMethodError"
end

puts "-- private_class_method, both spellings"
class E
  private_class_method def self.mk = 1
  def self.pub = 2
end
p E.singleton_methods(false).sort
p E.respond_to?(:mk)
p E.singleton_class.private_instance_methods(false).sort
