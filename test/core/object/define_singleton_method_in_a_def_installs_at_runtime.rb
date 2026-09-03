# `SomeClass.define_singleton_method(:name) { }` written INSIDE a method
# body (cannon and `by` retarget class methods mid-run; rails_admin
# intercepts Object.const_missing this way) is a runtime overlay install,
# not a class reopen -- there is no class-body site to register in a def.
# (Whether a runtime-installed `const_missing` HOOK is consulted by a
# folded constant read is the separate, documented divergence in
# const_missing_on_a_named_class.rb.)
class Router
  def self.routes
    "static routes"
  end
end

class App
  def boot(answer)
    Router.define_singleton_method(:routes) { "runtime #{answer}" }
  end
end

p Router.routes
App.new.boot(42)
p Router.routes
p Router.singleton_methods.include?(:routes)

# A fresh singleton method (no static row at all), with a capture.
class Gauge; end
def calibrate(offset)
  Gauge.define_singleton_method(:read) { 100 + offset }
end
calibrate(7)
p Gauge.read

# Receiverless inside a def installs on the method's own self.
class Widget
  def specialize
    define_singleton_method(:kind) { :special }
    self
  end
end
w = Widget.new.specialize
p w.kind
p Widget.new.respond_to?(:kind)
puts "still running"
__END__
"static routes"
"runtime 42"
true
107
:special
false
still running
