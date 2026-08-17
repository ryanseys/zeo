# A module included BOTH into a class and at toplevel is compiled twice, and
# the two instantiations do not agree on the return type of a method that
# constructs an object. One of them degrades to `int`, so the emitted C does
# not build:
#
#   error: incompatible types when returning type 'int'
#          but 'sp_Box' {aka 'struct sp_Box_s'} was expected
#   error: request for member 'iv_ch' in something not a structure or union
#
# Either include ALONE is fine -- `include Helpers` in the class without the
# toplevel one, or the toplevel one without the class, both compile and run.
# It is the combination that breaks.
#
# Found porting tobi/try (a Ruby CLI + TUI) to the Spinel subset: its TUI
# helper module is mixed into the screen classes and also included at toplevel
# so the script body can call the same helpers.
class Box
  attr_reader :ch
  def initialize(ch); @ch = ch; end
end

module Helpers
  def fill(ch); Box.new(ch); end
end

class Screen
  include Helpers          # included into a class...
  def go; fill("="); end
end

include Helpers            # ...AND at toplevel

puts Screen.new.go.ch
puts fill("-").ch

# a second method on the same module, to show it is not specific to one name
module More
  def wrap(ch); Box.new("[#{ch}]"); end
end

class Panel
  include More
  def draw; wrap("x"); end
end

include More

puts Panel.new.draw.ch
puts wrap("y").ch
