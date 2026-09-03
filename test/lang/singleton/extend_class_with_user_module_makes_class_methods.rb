module Greeter
  def hello; "hello from #{name}"; end
end
class Widget
  def self.name; "Widget"; end
  extend Greeter
end
# And the runtime call form on a plain class:
class Gadget; end
module M; def tag; "tagged"; end; end
Gadget.extend(M)
puts Widget.hello
puts Gadget.tag
__END__
hello from Widget
tagged
