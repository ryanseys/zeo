module Marker
  def tag = :marker
end

class Parent
  def label
    raise NotImplementedError
  end
  [:x].each { |n| define_method(:"dyn_#{n}") { n } }
end

class Child < Parent
  def label = :child
end

Child.prepend(Marker)
c = Child.new
p c.label
p c.tag
p Child.ancestors.first(3)
p Child.instance_method(:label).owner
__END__
:child
:marker
[Marker, Child, Parent]
Child
