# `Method#inspect` / `UnboundMethod#inspect` name where a method LIVES, how it
# was reached, and what it takes. The location tail is this file's path, so
# every assertion below strips it -- what is being pinned is the shape in
# front of it.
#
# Four divergences this file deliberately steps around, all in the rendering
# of methods zeo did not compile from Ruby source:
#
#   * CRuby reports a location for the parts of ITSELF written in Ruby
#     (`#<Method: Integer#zero?() <internal:numeric>:288>`); zeo's are native,
#     so they print unlocated.
#   * zeo's `Integer#zero?` lives on `Numeric`, so it prints
#     `Integer(Numeric)#zero?`.
#   * A per-object singleton (`def obj.m(x)`) is a runtime-defined body here,
#     carrying an arity but no parameter NAMES, so it prints `m()`.
#   * A native method with no declared arity prints the variadic `(*)` where
#     CRuby knows the real count.
#
# Everything below is a method zeo compiled, or a native one whose arity is
# declared -- the surface an inspect implementation is actually responsible for.

def shape(m) = m.inspect.sub(/ [^ ]+:\d+>\z/, ">")

# --- an instance method: its own class, then the ancestor that defines it ---
class Base
  def greet = "base"
end
class Sub < Base
  def greet = "sub"
end
class Quiet < Base; end

p shape(Sub.new.method(:greet))
p shape(Quiet.new.method(:greet))
p shape(Sub.new.method(:greet).super_method)

# A module in the chain is named the same way.
module Loud
  def shout = "!"
end
class Speaker
  include Loud
end
p shape(Speaker.new.method(:shout))

# --- the signature, in Ruby's own `def` spelling -----------------------------
class Sig
  def none = 1
  def req(a, b) = a
  def opt(a, b = 1) = a
  def rest(a, *r) = a
  def kw(a, k:, j: 2) = a
  def kwrest(**kw) = kw
  def blk(&b) = b
  def every(a, b = 1, *r, c, k:, j: 2, **kw, &blk) = a
end
%i[none req opt rest kw kwrest blk every].each do |name|
  p shape(Sig.new.method(name))
end

# --- a class method prints a `.`, and qualifies an inherited one -------------
module Api
  def self.fetch(url) = url
end
class Store
  def self.open(path, mode = "r") = path
end
class Cache < Store; end

p shape(Api.method(:fetch))
p shape(Store.method(:open))
p shape(Cache.method(:open))

# --- a per-object singleton names the RECEIVER, not a class ------------------
class Widget
  def to_s = "a widget"
  def inspect = "#<Widget custom>"
end
w = Widget.new
def w.only = 1
p shape(w.method(:only))

# --- natively defined methods: arity placeholders, and no location -----------
p [].method(:each).inspect
p "".method(:sub).inspect
p 1.method(:+).inspect
p Hash.instance_method(:store).inspect
p Array.method(:try_convert).inspect

# --- unbound: the OWNER alone, never the class it was fetched from -----------
p shape(Sub.instance_method(:greet))
p shape(Quiet.instance_method(:greet))
p shape(Speaker.instance_method(:shout))
p shape(Sig.instance_method(:every))
p shape(Sub.new.method(:greet).unbind)
p shape(method(:shape).unbind)

# --- a top-level def lives on Object -----------------------------------------
def helper(n, verbose: false) = n
p shape(method(:helper))
p shape(Object.instance_method(:helper))

# `to_s` is the same string.
p Sub.new.method(:greet).to_s == Sub.new.method(:greet).inspect
p Sub.instance_method(:greet).to_s == Sub.instance_method(:greet).inspect
