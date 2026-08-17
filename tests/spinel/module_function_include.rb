module MF
  module_function
  def helper(x); x * 2; end
  def greet; "hi"; end
end
class Use
  include MF
  def go; helper(3); end
  def hello; greet; end
end
p Use.new.go
p Use.new.hello
p MF.helper(3)
p MF.greet

module MF2
  def named(x); x + 1; end
  module_function :named
end
class Use2
  include MF2
  def go; named(1); end
end
p Use2.new.go
p MF2.named(1)

module Util
  module_function
  def twice(n); n * 2; end
end
class Base
  include Util
end
class Child < Base
  def go; twice(5); end
end
p Child.new.go

module Sh
  module_function
  def label; "mod"; end
end
class Own
  include Sh
  def label; "own"; end
  def go; label; end
end
p Own.new.go
