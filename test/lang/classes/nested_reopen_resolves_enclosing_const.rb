# A class first defined COMPACT (`class Outer::Widget`) and later reopened
# NESTED (`module Outer; class Widget`) must resolve a bare constant against the
# reopen's enclosing nesting (`Outer::Helper`), not just the compact def's own
# scope -- the rubygems `Gem::StubSpecification` / `prepend BetterPermissionError`
# shape.
module Outer
end

class Outer::Widget
end

module Outer
  module Helper
    def hi = "helper"
  end

  class Widget
    prepend Helper
  end
end

puts Outer::Widget.new.hi
__END__
helper
