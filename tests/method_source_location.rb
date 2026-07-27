# `Method#source_location` / `UnboundMethod#source_location` -- `[file, line]`
# for a method written in Ruby, `nil` for one defined natively. The line is the
# `def` keyword's own, which is why the assertions below name it explicitly
# rather than printing the (machine-specific) path.

def at_line_6 = 1

class Shapes
  def area(w, h)              # line 9
    w * h
  end

  def self.build = new        # line 13

  attr_accessor :label        # line 15

  alias_method :size, :area   # line 17
end

def line_of(m) = m.source_location&.last
def file_of(m) = File.basename(m.source_location&.first || "")

# A plain top-level def, a class body def, and a class method.
p line_of(method(:at_line_6))
p line_of(Shapes.new.method(:area))
p line_of(Shapes.method(:build))

# The file is this one, in every case.
p file_of(method(:at_line_6))
p file_of(Shapes.new.method(:area))

# An attr_accessor pair reports the `attr_accessor` line itself.
p line_of(Shapes.new.method(:label))
p line_of(Shapes.new.method(:label=))

# An alias reports its SOURCE's location, not the alias line.
p line_of(Shapes.new.method(:size))

# UnboundMethod answers the same pair.
p line_of(Shapes.instance_method(:area))
p Shapes.instance_method(:area).source_location.is_a?(Array)
p Shapes.instance_method(:area).source_location.length

# A method defined at runtime carries its block's location.
class Shapes
  define_method(:scaled) { |k| k }   # line 46
end
p line_of(Shapes.new.method(:scaled))

# Natively defined methods have no Ruby source.
p 5.method(:+).source_location
p [].method(:push).source_location
p Array.method(:try_convert).source_location
p String.instance_method(:upcase).source_location
