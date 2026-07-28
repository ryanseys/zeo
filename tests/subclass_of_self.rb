# `class Sub < self` inside a class body names the ENCLOSING class -- a
# compile-time fact, not a dynamic superclass. optparse gives every argument
# style this shape (`class NoArgument < self` inside `class Switch`), and
# rubygems vendors optparse, so every `gem` command depends on it.
class Switch
  def initialize(pattern = nil, conv = nil, &block)
    @pattern, @conv, @block = pattern, conv, block
  end
  attr_reader :pattern, :conv, :block

  def self.guess(arg) = arg.nil? ? NoArgument : RequiredArgument

  class NoArgument < self
    def self.pattern = /\A\z/
    def parse(arg) = [:none, arg]
  end

  class RequiredArgument < self
    def parse(arg) = [:required, arg]
  end
end

s = Switch::NoArgument.new { :from_block }
p s.class, s.block.call, s.parse(1)
p Switch::RequiredArgument.new(/x/).pattern
p Switch::RequiredArgument.new(/x/).parse(2)
p Switch.new(nil, :conv).conv

# The inherited surface, the class-method surface, and the ancestry.
p Switch::NoArgument.ancestors[0, 2]
p Switch::NoArgument.superclass
p Switch::NoArgument.pattern
p Switch.guess(nil), Switch.guess(1)
p Switch::NoArgument.new.is_a?(Switch)
p Switch::NoArgument.instance_method(:conv).owner

# Held in a collection and dispatched dynamically, the way optparse's
# DefaultList does.
list = {}
list["-"] = Switch::NoArgument.new { :dash }
list["="] = Switch::RequiredArgument.new(/=/) { :eq }
p list.map { |k, v| [k, v.class, v.parse(k)] }

# A deeper chain: `< self` from inside a subclass body names THAT subclass.
class Base
  def kind = :base
  class Middle < self
    def kind = :middle
    class Leaf < self
      def kind = :leaf
    end
  end
end
p Base::Middle::Leaf.ancestors[0, 3]
p Base::Middle::Leaf.new.kind
p Base::Middle.new.kind

# `super` still walks the chain the header established.
class Chain
  def describe = "chain"
  class Link < self
    def describe = "link of #{super}"
  end
end
p Chain::Link.new.describe
