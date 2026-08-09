# `class X < Regexp` is a ValueSubclass husk wrapping a real Regexp. Unlike a
# String or Array subclass, ruby has no conversion protocol for it, so every
# site that takes a pattern has to see through the husk itself.

# verbal_expressions' shape: build the source in `initialize`, seat it with
# `super`.
class MyRe < Regexp
  def initialize(src)
    super("(#{src})")
  end
end

r = MyRe.new("a+")
p r.class
p r.source
p r.inspect
p r.is_a?(Regexp)

# As a RECEIVER -- the payload bridge answers these.
p r =~ "xxaaay"
p r.match("xxaaay")[0]
p r.match?("xxaaay")
p r === "xxaaay"

# As an ARGUMENT, which is the half that needed the unwrapping.
p "xxaaay" =~ r
p "xxaaay".match(r)[1]
p "xxaaay".match?(r)
p "xxaaay"[r]
p "xxaaay"[r, 1]
p "xxaaay".gsub(r, "Z")
p "xxaaay".sub(r) { |m| m.upcase }
p "xxaaay".scan(r)
p "a1a2a".split(r)
p "xxaaay".index(r)
p "xxaaay".rindex(r)
p "xxaaay".byteindex(r)
p "xxaaay".start_with?(MyRe.new("x"))
p "xxaaay".partition(r)
p "xxaaay".slice!(r)

case "xxaaay"
when r then p :matched
else p :no
end

# Through the Regexp class methods.
p Regexp.union(r, /b/)
p Regexp.new(r).source
p Regexp.linear_time?(r)

# Identity and hashing go by pattern, but the object stays a MyRe.
p r == /(a+)/
p({ r => 1 }.keys.first.class)

# riel's shape: override a match method and call `super`.
class NegatedRegexp < Regexp
  def match(str)
    !super
  end
end
n = NegatedRegexp.new("z")
p n.match("abc")
p n.match("zoo")

# The pattern reaches every OTHER pattern-taking site the same way.
require 'strscan'
p ["aaa", "b"].grep(r)
p ["aaa", "b"].grep_v(r)
sc = StringScanner.new("aaab")
p sc.scan(r)
p sc.check(MyRe.new("b"))
p sc.skip(MyRe.new("b"))
t = +"xxaaay"
t[r] = "Z"
p t
p "aaa".rpartition(r)
p Regexp.try_convert(r).class
p r.hash == /(a+)/.hash
p r.eql?(/(a+)/)
# A subclass marshals as its own class, with the source and flags a bare
# Regexp carries.
p Marshal.load(Marshal.dump(r)).class
p Marshal.load(Marshal.dump(r)).source
