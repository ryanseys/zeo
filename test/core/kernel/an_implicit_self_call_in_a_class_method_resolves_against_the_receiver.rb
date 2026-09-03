# `self` in a class method is the class it was CALLED on, not the one
# whose body the method was written in. Both of these were silently wrong
# (no error, just the wrong object) while implicit-self resolution used
# the lexical `defining_class`:
# - `Sub.create` built a Base;
# - `Ext.helped` answered "Helper".

class Base
  def self.create; new; end
  def self.who; name; end
end
class Sub < Base; end
p Base.create.class
p Sub.create.class
p Sub.who

module Helper
  def helped; "helped-#{name}"; end
end
class Ext
  extend Helper
end
p Ext.helped
__END__
Base
Sub
"Sub"
"helped-Ext"
