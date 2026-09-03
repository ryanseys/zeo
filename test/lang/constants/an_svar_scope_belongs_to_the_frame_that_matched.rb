# `$~` -- and every read that goes through it (`$1`, `$&`, `` $` ``, `$'`,
# `Regexp.last_match`) -- is FRAME-LOCAL. A scope that matches and returns
# leaves the caller's match exactly as it found it, and a scope that never
# matched reads the one it inherited from where it was ENTERED.
#
# The rule follows the frame: a method, a class or module body and a
# singleton-class body each get a scope of their own; a block, a lambda and a
# `define_method` body share the scope of the frame they run under, because
# that is the frame ruby gives them.
#
# CLIF pushed no svar scope at all, so every callee's match reached its
# caller. The rustc backend pushed one for methods and not for class bodies.

probe = -> { [$~ && $~[0], $1] }

# --- a method keeps its match to itself ------------------------------------
def matches(s)
  s =~ /(y)/
  $1
end
"abc" =~ /(b)/
p matches("xyz")
p probe.call

# A method that matches without ever naming an svar still needs the scope.
def silent(s)
  s.match(/(y)/)
  :done
end
"abc" =~ /(b)/
p silent("xyz")
p probe.call

# --- a class body is a frame, so it is a scope -----------------------------
"abc" =~ /(b)/
class Body
  "xyz" =~ /(y)/
  INSIDE = $1
end
p Body::INSIDE
p probe.call

"abc" =~ /(b)/
module Mod
  "xyz" =~ /(y)/
end
p probe.call

class Sing
  class << self
    "xyz" =~ /(y)/
  end
end
p probe.call

# --- a block shares the frame it runs under --------------------------------
"abc" =~ /(b)/
[1].each { "xyz" =~ /(y)/ }
p probe.call

"abc" =~ /(b)/
-> { "xyz" =~ /(y)/ }.call
p probe.call

"abc" =~ /(b)/
Class.new { "xyz" =~ /(y)/ }
p probe.call

# ...but a block inside a METHOD writes that method's scope, not ours.
def through_a_block(s)
  [1].each { s =~ /(y)/ }
  $1
end
"abc" =~ /(b)/
p through_a_block("xyz")
p probe.call

# --- a class method and a `define_method` body -----------------------------
class Owner
  def self.classy(s)
    s =~ /(y)/
    $1
  end
  define_method(:installed) do |s|
    s =~ /(y)/
    $1
  end
end
"abc" =~ /(b)/
p Owner.classy("xyz")
p probe.call
p Owner.new.installed("xyz")
p probe.call

# --- a scope that fails to match reports the failure, not the inherited one -
def fails(s)
  s =~ /(q)/
  [$~, $1]
end
"abc" =~ /(b)/
p fails("xyz")
p probe.call

# --- an early return cannot leak the callee's match ------------------------
def early(s)
  return :none unless s =~ /(y)/
  $1
end
"abc" =~ /(b)/
p early("zzz")
p probe.call

# --- a raise out of a matched scope pops it too ----------------------------
def raises(s)
  s =~ /(y)/
  raise "boom"
end
"abc" =~ /(b)/
begin
  raises("xyz")
rescue RuntimeError
  p probe.call
end
__END__
"y"
["b", "b"]
:done
["b", "b"]
"y"
["b", "b"]
["b", "b"]
["b", "b"]
["y", "y"]
["y", "y"]
["y", "y"]
"y"
["b", "b"]
"y"
["b", "b"]
"y"
["b", "b"]
[nil, nil]
["b", "b"]
:none
["b", "b"]
["b", "b"]
