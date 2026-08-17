# A method whose body ends in a call nothing resolves (`@text.string` with
# @text a String) never returns a value: the emitter answers that tail with a
# NoMethodError raise. A caller that READS the value still needs a typed
# result, and the untyped return made the assignment a void expression.

# A user class sharing the tail call's NAME is not a candidate when the
# receiver is statically a String: only a reopen of String could answer it.
class Holder
  def initialize
    @string = "held"
  end

  def string
    @string
  end
end

class Gen
  def initialize
    @text = ""
  end

  def text
    @text.string
  end
end

class Parse
  def initialize
    @t = ""
  end

  def prep
    g = Gen.new
    @t = g.text
  end

  def size
    @t.length
  end
end

p1 = Parse.new
begin
  p1.prep
rescue NoMethodError => e
  puts "raised"
end
p p1.size
p Holder.new.string
