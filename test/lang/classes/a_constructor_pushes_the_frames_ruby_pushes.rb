# `Foo.new` pushes NO backtrace frame of its own. zeo pushed `Class#new` for
# every construction, so a raise from any `initialize` reported one frame ruby
# does not have.
#
# Two shapes DO frame, and each is a different mechanism rather than an
# exception to one rule:
#
#   * `Class.new { }` / `Module.new { }` MINTING -- the receiver is `Class`
#     itself and the block IS the body, so ruby reports `Class#initialize`
#     then `Class#new`;
#   * a `Data` class, whose `new` is a distinct cfunc and reports as `D.new`.
#
# The rule is therefore about the RECEIVER, where zeo's frame-label table is
# keyed by the row's OWNER -- and the owner is `Class` in every case above.
# So the row is silenced in that table and the two framing shapes push their
# own.
#
# Every row below is oracle-verified against ruby 4.0.6.

def frames(label, n = 2)
  yield
rescue => e
  puts "#{label}:"
  puts e.backtrace.first(n).map { |l| "  " + l.sub(%r{\A.*?([^/]+):}, '\1:') }
end

# --- the ordinary case: no frame for `new`
class P1
  def initialize(a)
    raise "plain"
  end
end
frames("plain class") { P1.new(1) }

# --- a Struct subclass: also no frame
S = Struct.new(:a)
class S2 < S
  def initialize(*a)
    super
    raise "struct"
  end
end
frames("struct subclass") { S2.new(1) }

# --- an exception subclass: also no frame
class E1 < StandardError
  def initialize(m = "e")
    super
    raise "exception"
  end
end
frames("exception subclass") { E1.new }

# --- a class with no `initialize` at all
class P2; end
frames("arity error, no initialize") { P2.new(1, 2) }

# --- MINTING does frame, both spellings
frames("Class.new minting", 3) { Class.new { raise "mint" } }
frames("Module.new minting", 3) { Module.new { raise "mint" } }

# --- a subclass whose superclass is an ordinary compiled class
class P3 < P1
  def initialize(a)
    super
  end
end
frames("subclass calling super") { P3.new(1) }

# --- `allocate` skips `initialize` entirely, so nothing to frame
frames("allocate then raise") { P1.allocate.instance_variable_get(:@nope) || raise("after allocate") }
__END__
plain class:
  a_constructor_pushes_the_frames_ruby_pushes.rb:30:in 'P1#initialize'
  a_constructor_pushes_the_frames_ruby_pushes.rb:33:in 'block in <main>'
struct subclass:
  a_constructor_pushes_the_frames_ruby_pushes.rb:40:in 'S2#initialize'
  a_constructor_pushes_the_frames_ruby_pushes.rb:43:in 'block in <main>'
exception subclass:
  a_constructor_pushes_the_frames_ruby_pushes.rb:49:in 'E1#initialize'
  a_constructor_pushes_the_frames_ruby_pushes.rb:52:in 'block in <main>'
arity error, no initialize:
  a_constructor_pushes_the_frames_ruby_pushes.rb:56:in 'BasicObject#initialize'
  a_constructor_pushes_the_frames_ruby_pushes.rb:56:in 'block in <main>'
Class.new minting:
  a_constructor_pushes_the_frames_ruby_pushes.rb:59:in 'block (2 levels) in <main>'
  a_constructor_pushes_the_frames_ruby_pushes.rb:59:in 'Class#initialize'
  a_constructor_pushes_the_frames_ruby_pushes.rb:59:in 'Class#new'
Module.new minting:
  a_constructor_pushes_the_frames_ruby_pushes.rb:60:in 'block (2 levels) in <main>'
  a_constructor_pushes_the_frames_ruby_pushes.rb:60:in 'Module#initialize'
  a_constructor_pushes_the_frames_ruby_pushes.rb:60:in 'Class#new'
subclass calling super:
  a_constructor_pushes_the_frames_ruby_pushes.rb:30:in 'P1#initialize'
  a_constructor_pushes_the_frames_ruby_pushes.rb:65:in 'P3#initialize'
allocate then raise:
  a_constructor_pushes_the_frames_ruby_pushes.rb:71:in 'block in <main>'
  a_constructor_pushes_the_frames_ruby_pushes.rb:21:in 'Object#frames'
