# `main`'s private singleton methods. CRuby installs eight on the top-level
# self (`define_method`, `include`, `inspect`, `private`, `public`,
# `ruby2_keywords`, `to_s`, `using`); each of the mixin ones lands on
# `Object`, which is what makes what it does visible everywhere afterwards.
#
# zeo installs no singleton on main at all -- doing it at startup would mark
# the runtime-overlay maps live for every program -- so dispatch routes the
# names instead. Only `include` was routed, and a bare `private` at top level
# (plain Ruby, and the shape rubygems' own files open with) was a
# NoMethodError naming `main`.
#
# Reflection reads the same list, so `respond_to?`, `private_methods` and
# `singleton_methods` cannot drift from what dispatch answers.

def helper = "helper"
private :helper
puts "private with a name  #{Object.private_method_defined?(:helper)}"
puts "still callable       #{helper}"

def public_one = "public"
public :public_one
puts "public with a name   #{Object.public_method_defined?(:public_one)}"
puts "through self         #{self.public_one}"

private
def after_bare = "after"
puts "after bare private   #{Object.private_method_defined?(:after_bare)}"
puts "still callable       #{after_bare}"

define_method(:from_define) { "from_define" }
puts "define_method        #{from_define}"

module Mixed
  def from_mixed = "from_mixed"
end
include Mixed
puts "include at top       #{from_mixed}"
puts "reached Object       #{Object.ancestors.include?(Mixed)}"

puts "to_s                 #{self.to_s}"
puts "inspect              #{self.inspect}"
puts "public singletons    #{self.singleton_methods.sort}"
puts "respond_to? private  #{respond_to?(:private, true)}"
puts "respond_to? public   #{respond_to?(:public, true)}"
puts "not without all      #{respond_to?(:private)}"
puts "private_methods      #{(self.private_methods & %i[private public include define_method]).sort}"
__END__
private with a name  true
still callable       helper
public with a name   true
through self         public
after bare private   true
still callable       after
define_method        from_define
include at top       from_mixed
reached Object       true
to_s                 main
inspect              main
public singletons    [:inspect, :to_s]
respond_to? private  true
respond_to? public   true
not without all      false
private_methods      [:define_method, :include, :private, :public]
