# The two halves of an alias's identity, which pull in opposite directions.
#
# A FRAME names the method it was BORN as: CRuby labels it from
# `me->def->original_id`, so a backtrace through `alias_method :kaboom, :boom`
# says `C#boom`. zeo labelled it with the alias.
#
# `__callee__` is the other half and keeps the name the method was CALLED by,
# while `__method__` keeps the original. Those two shared one string -- the
# frame label -- so fixing the frame broke `__callee__`, and it is folded at
# compile time now: an alias is emitted as its own body, which knows the name
# it answers to.

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

class Both
  def orig(x) = [__method__, __callee__, x]
  alias_method :by_method, :orig
  alias by_keyword orig
  def frame = caller(0).first[/in \'(.*)\'/, 1]
  alias_method :frame_alias, :frame
end
p Both.new.orig(1)
p Both.new.by_method(2)
p Both.new.by_keyword(3)
p [Both.new.frame, Both.new.frame_alias]
p [Both.instance_method(:by_method).original_name, Both.new.method(:by_method).name]

module Shared
  def base = [__method__, __callee__, caller(0).first[/in \'(.*)\'/, 1]]
  alias_method :aliased, :base
end
class Host
  include Shared
end
p Host.new.base
p Host.new.aliased

class Deep
  def one = __callee__
  alias two one
  alias three two
end
p [Deep.new.one, Deep.new.two, Deep.new.three]
__END__
[:real, :real, 1]
[:real, :nick, 2]
[:real, :nick2, 3]
[:real, :nick]
lang/methods/an_alias_frame_names_the_method_it_was_born_as.rb:17:in 'Al#boom'
["Mixin#base", "Mixin#base"]
[:orig, :orig, 1]
[:orig, :by_method, 2]
[:orig, :by_keyword, 3]
["Both#frame", "Both#frame"]
[:orig, :by_method]
[:base, :base, "Shared#base"]
[:base, :aliased, "Shared#base"]
[:one, :two, :three]
