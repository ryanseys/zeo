class Al
  def real(x) = [__method__, __callee__, x]
  alias_method :nick, :real
  alias nick2 real
  def boom = raise("x")
  alias_method :kaboom, :boom
end

p Al.new.real(1)
p Al.new.nick(2)
p Al.new.nick2(3)
p [Al.instance_method(:nick).original_name, Al.new.method(:nick).name]

begin
  Al.new.kaboom
rescue RuntimeError => e
  puts e.backtrace.first
end

module Mixin
  def base = caller(0).first[/in '(.*)'/, 1]
  alias_method :aliased, :base
end
class User
  include Mixin
end
p [User.new.base, User.new.aliased]
