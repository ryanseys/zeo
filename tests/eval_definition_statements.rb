# The definition-level statements a run-time `eval` may write. Each one names
# a DEFINEE only the run time knows -- `class_eval`'s receiver, a plain
# `eval`'s enclosing class -- so each is the send ruby writes on it, and a
# bare `private`/`module_function` is a cursor every later `def` reads.

class K
  def a = "a"
  def b = "b"
  def self.c = "c"
end

p K.class_eval("alias a2 a".dup)
p K.new.a2
K.class_eval("undef b".dup)
p K.new.respond_to?(:b)
p K.class_eval("private :a".dup)
p [K.new.respond_to?(:a), K.new.send(:a)]
K.class_eval("private_class_method :c".dup)
p K.respond_to?(:c)

# A bare directive is the body's running default.
class Cursor; end
Cursor.class_eval("def pub = 1\nprivate\ndef pri = 2\npublic\ndef pub2 = 3".dup)
p [Cursor.new.respond_to?(:pub), Cursor.new.respond_to?(:pri), Cursor.new.respond_to?(:pub2)]
p Cursor.new.send(:pri)

module Funcs
  def helper = :h
end
p Funcs.module_eval("module_function :helper".dup)
p Funcs.helper

module Bare; end
Bare.module_eval("module_function\ndef mf = :mf".dup)
p [Bare.mf, Bare.respond_to?(:mf)]

# `include`/`extend`/`prepend` are ordinary sends, hooks and all.
module Loud
  def self.included(base) = puts("included #{base}")
  def shout = "loud"
end
class Host; end
p Host.class_eval("include Loud".dup)
p Host.new.shout

# A redefinition inside one snippet takes at its own position.
class Twice; end
Twice.class_eval("def d = 1\ndef d = 2".dup)
p Twice.new.d

# A `def` announces to the class it lands on.
class Watched
  def self.method_added(name) = puts("added #{name}")
end
Watched.class_eval("def watched = 1".dup)

# A singleton class is an ordinary definee too.
class Single
  class << self
    def sc = 1
  end
end
Single.singleton_class.class_eval("alias sc2 sc".dup)
p Single.sc2
