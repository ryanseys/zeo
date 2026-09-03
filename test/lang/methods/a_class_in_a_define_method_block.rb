# Ruby rejects the `class` keyword inside a `def`, and accepts it inside a
# BLOCK -- so `define_method(:x) { module M; end }` is legal, and the module
# takes the block's own lexical cref. zeo desugars a literal-symbol
# `define_method` into the same node a `def` produces, which made the walk
# stop there and the definition reach codegen unregistered.
#
# followupboss_client is the shape at the top level (a `require` override
# whose branches define `Her::Middleware` classes), import.rb the shape in a
# class body (`module ::Kernel` inside `define_method(:global)`).

# The body runs when the METHOD does, not when it is defined -- and only
# then, once per call.
define_method(:install_top) do
  module Top
    puts "top body"
    WHO = "top"
    def self.hi = "top hi"
  end
end

puts "defined"
install_top
p [Top::WHO, Top.hi, Top.class]

# The cref is the block's, so a class body's `define_method` nests the
# definition under that class.
class Host
  define_method(:install) do
    module Inner
      WHO = "inner"
    end

    class Nested
      def hi = "nested"
    end
  end

  define_singleton_method(:install_from_singleton) do
    module FromSingleton
      WHO = "singleton"
    end
  end
end

Host.new.install
Host.install_from_singleton
p [Host::Inner::WHO, Host::Nested.new.hi, Host::Nested.name]
p [Host::FromSingleton::WHO, defined?(Inner), defined?(FromSingleton)]

# A cref-absolute reopen from inside one reaches the real top-level class,
# and a branch inside the block is descended into just as at the top level.
# (No `defined?` probe before the call: zeo registers a definition at compile
# time whatever position it is written in, so `defined?(1.zeo_shout)` answers
# "method" before the block has run where ruby answers nil. That divergence
# belongs to the registration model and predates this shape -- a plain block
# shows it too.)
define_method(:patch) do |which|
  case which
  when :kernel
    module ::Kernel
      def zeo_shout = "SHOUT"
    end
  when :string
    class ::String
      def zeo_shout = "str shout"
    end
  end
end

patch(:kernel)
patch(:string)
p [1.zeo_shout, "s".zeo_shout]

# Calling twice re-runs the body, which is what makes the counter climb --
# the same thing a plain block does. (A `module` reopen, so no constant is
# assigned twice: the second assignment's warning goes to stderr, and the
# order it interleaves with stdout is not a fact about either compiler.)
module Counter
  def self.n = @n ||= 0
  def self.bump = @n = n + 1
end
define_method(:count_up) do
  module Counter
    bump
  end
end
count_up
count_up
p Counter.n
__END__
defined
top body
["top", "top hi", Module]
["inner", "nested", "Host::Nested"]
["singleton", nil, nil]
["SHOUT", "str shout"]
2
