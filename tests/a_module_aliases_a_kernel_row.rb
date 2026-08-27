# `alias` inside a bare MODULE finds a method of `Object`.
#
# A module's ancestry is itself alone, so a `Kernel` row is not in it -- and
# CRuby's `rb_alias` retries the lookup on `Object` for exactly that case
# (`vm_method.c`, the `RB_TYPE_P(klass, T_MODULE)` branch before
# `rb_print_undef`). Zeo checked only the ancestry and raised `NameError` at
# program start.
#
# irb opens `IRB::IrbLoader` with two of them -- `alias ruby_load load` and
# `alias ruby_require require` -- so `require "irb"` died before doing
# anything.
#
# A CLASS gets no such retry: its ancestry already reaches `Object`, and a
# name that is not there is a real NameError.

module Loader
  alias ruby_load load
  alias ruby_require require
  alias say puts

  def announce = say("via the alias")
end

class Host
  include Loader
end

Host.new.announce
p Loader.private_instance_methods(false).sort
p Host.new.respond_to?(:ruby_load)
p Host.new.respond_to?(:ruby_load, true)

# The retry is for a MISSING name only: a module's own method still wins.
module Own
  def real = "own"
  alias copy real
end
p Own.instance_method(:copy).owner
p Class.new { include Own }.new.copy

# A name nowhere at all is still NameError, module or not.
begin
  Module.new { alias nope no_such_method_anywhere }
rescue NameError => e
  puts e.message.sub(/for module '.*'/, "for module '<anon>'")
end
