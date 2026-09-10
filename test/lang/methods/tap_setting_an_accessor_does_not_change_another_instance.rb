# `Crash.new.tap { _1.foo = true }` leaves a second instance's foo nil.
class Crash
  attr_accessor :foo
  def initialize = @foo = nil
end

Crash.new.tap { _1.foo = true }

p Crash.new.foo
__END__
nil
