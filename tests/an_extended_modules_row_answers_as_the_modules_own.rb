# `extend M` seats M in the SINGLETON class's ancestry: the class method it
# supplies is still M's own instance method, and every reflection answer says
# so. zeo scanned only the class ancestry for a class-method owner, so it
# found nothing there and fell back to reporting `Class`.
module Ext
  def hi = 1
end
class K
  extend Ext
  def self.own = 2
end

m = K.method(:hi)
p m.owner
p m.arity, m.name, m.receiver
p m.unbind.owner
p m.super_method
p m.call

# An own `def self.x` is the other shape and must keep its own answers: it
# really is minted on the singleton class.
p K.method(:own).owner
p K.singleton_methods(false), K.singleton_methods.sort
p K.singleton_class.instance_methods(false)

# `module M; def self.x; end; end` is NOT the extend shape either -- `M` owns
# a real class method there.
module Mod
  def self.mown = 3
end
p Mod.method(:mown).owner

# A runtime `extend` answers the same way as one written in a class body.
class L
end
L.extend(Ext)
p L.hi, L.method(:hi).owner, L.respond_to?(:hi)
o = Object.new
o.extend(Ext)
p o.hi, o.respond_to?(:hi), o.is_a?(Ext), o.singleton_class.ancestors.take(2)

# The builtin that needed this: `CGI` both includes and extends `CGI::Escape`,
# which prepends `CGI::EscapeExt`, so no escape is a class method of `CGI`.
require "cgi/escape"
p CGI.method(:escapeHTML).owner
p CGI.method(:escapeElement).owner
p CGI.method(:h).owner
p CGI.singleton_class.instance_method(:escape).owner
p CGI.singleton_class.ancestors.take(3)
p CGI.singleton_class.instance_methods(false)
p CGI.instance_method(:escapeHTML).owner
p CGI.escapeHTML("<a>"), CGI.escapeElement("<A>", "A"), CGI.respond_to?(:h)
