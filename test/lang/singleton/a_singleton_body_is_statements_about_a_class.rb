# `class << Klass` opens the class's SINGLETON, so `prepend M` there puts M's
# instance methods in front of `Klass`'s own class methods, with `super` resuming
# at the original. The body is ordinary statements: `self` names the singleton
# class, an explicit `self.` receiver names the same thing, and a statement that
# never reaches for `self` means what it means anywhere and just runs.
#
# The active_hash gems spell their pagination hook `class << Base;
# self.prepend(ClassMethods); end`.

SEEN = []

module Wrap
  def greet = "wrapped(#{super})"
end

class Direct
  def self.greet = "Direct"
end

Direct.singleton_class.prepend(Wrap)
p Direct.greet

# The same edit written as a singleton body. It used to reach neither the
# ancestry recorder nor the statement stream, and vanished without a word.
class Opened
  def self.greet = "Opened"
end

class << Opened
  prepend Wrap
end

p Opened.greet

# (What a class-method prepend does NOT yet do is show up in
# `singleton_class.ancestors` or in `Method#owner` -- dispatch and `super`
# honour it, reflection does not. Filed separately; asserting it here would
# only pin the divergence.)

# An explicit `self.` receiver is that same singleton class.
module Paginatable
  module ClassMethods
    def page = "paged(#{super})"
  end
end

class Records
  def self.page = "records"
end

class << Records
  self.prepend(Paginatable::ClassMethods)
end

p Records.page

# A statement that consults `self` nowhere runs unchanged, at this position.
class Quiet
  def self.tag = :quiet
end

class << Quiet
  SEEN << :body_ran
  prepend Wrap
end

p SEEN
p Quiet.tag

# A bare `singleton_class` in a class body IS that class's singleton, with no
# constant naming it. activesupport writes its ERB patch that way, and pundit --
# and every gem behind activesupport -- reaches the ledger through it.
class Bare
  def self.greet = "bare"

  singleton_class.prepend Wrap
end

p Bare.greet

module Namespaced
  class Deep
    def self.greet = "deep"

    singleton_class.prepend Wrap
  end
end

p Namespaced::Deep.greet

# And the per-object form still defines per-object methods.
obj = Object.new

class << obj
  def shout = :loud
end

p obj.shout
p obj.singleton_methods
__END__
"wrapped(Direct)"
"wrapped(Opened)"
"paged(records)"
[:body_ran]
:quiet
"wrapped(bare)"
"wrapped(deep)"
:loud
[:shout]
