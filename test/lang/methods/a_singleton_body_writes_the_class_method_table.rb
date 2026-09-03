# A `class << self` body's statements are RECEIVERLESS in ruby. zeo rebinds
# them onto `self.singleton_class`, so the receiver must be the surrogate --
# that is the table those methods live in -- while the visibility barrier the
# receiverless form never raises must stay down.

class Vis
  class << self
    def a = :a
    def b = :b
    def c = :c

    # The literal form folds at compile time; the computed one is a real
    # `Module#private`/`#public` send at run time. Both write the same table.
    private :a
    names = [:b]
    private(*names)
    public(*names)
  end
end
p Vis.b
p Vis.c
begin
  Vis.a
rescue NoMethodError => e
  puts e.message
end

# `extend self` first, then the singleton body re-scopes what the extend
# brought in -- fileutils' Verbose/NoWrite/DryRun shape.
module Chatty
  def speak = :spoke
  def hush = :hushed
  private :speak, :hush

  extend self

  class << self
    public(*[:speak, :hush])
  end
end
p Chatty.speak
p Chatty.hush

# NOTE: `protected(*names)` on a CLASS method is a known gap in both
# backends -- the runtime's class-method visibility is a private/public
# boolean, so protected cannot be recorded there. Left out deliberately
# rather than pinned wrong.

# A receiver the SOURCE wrote is not the synthesized one: the barrier holds.
class Hand
  class << self
    def open_it = :open

    private

    def shut = :shut
  end
end
p Hand.open_it
begin
  Hand.singleton_class
  Hand.shut
rescue NoMethodError => e
  puts e.message
end
__END__
:b
:c
private method 'a' called for class Vis
:spoke
:hushed
:open
private method 'shut' called for class Hand
