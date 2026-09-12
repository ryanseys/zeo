# A syntax-tree node takes to_s from Kernel while owning inspect, a Location
# reaches new through Class, and so does a Monitor.
N = RubyVM::AbstractSyntaxTree::Node
L = RubyVM::AbstractSyntaxTree::Location
p N.instance_methods(false).include?(:to_s), N.instance_method(:to_s).owner.to_s
p N.instance_methods(false).include?(:inspect), N.instance_method(:inspect).owner.to_s
p L.singleton_methods(false).include?(:new), L.method(:new).owner.to_s
require "monitor"
p Monitor.singleton_methods(false).include?(:new), Monitor.method(:new).owner.to_s
m = Monitor.new
p m.mon_owned?, m.synchronize { 7 }
__END__
false
"Kernel"
true
"RubyVM::AbstractSyntaxTree::Node"
false
"Class"
false
"Class"
false
7
