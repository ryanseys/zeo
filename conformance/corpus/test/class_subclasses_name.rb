# Class#subclasses hands back boxed classes, so `.map { |c| c.name }` reaches
# the poly dispatch: a class-tagged value answers #name/#to_s/#inspect with its
# class name (#2656). The tag is checked ahead of the cls_id switch, since a
# boxed class carries the CLASS's id and would otherwise alias that user arm.
class Base; end
class Kid1 < Base; end
class Kid2 < Base; end
p Base.subclasses.length
p Base.subclasses.map { |c| c.name }.sort
p Base.subclasses.map { |c| c.to_s }.sort
p Kid1.ancestors.map { |c| c.to_s }.first(3)

# a user #name still wins: the user object is the likelier receiver
class Person
  attr_reader :name
  def initialize(n); @name = n; end
end
p [Person.new("ann"), Person.new("bob")].map { |x| x.name }

# encoding-tagged values keep answering for themselves
p "x".encoding.name
p "x".encoding.to_s
