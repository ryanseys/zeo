# CRuby compiles an `attr_*` accessor iseq-less, so it appears in no
# backtrace. zeo's is frameless too -- but it still stamped its own line, and
# a stamp with no frame of its own lands in the CALLER's and is never
# restored: every later raise in `boom` reported the `attr_reader` line.
class Holder
  attr_reader :a
  attr_writer :w
  attr_accessor :b

  def initialize
    @a = 1
    @b = 2
  end

  def boom
    a.nope
  end

  def wboom
    self.w = 1
    freeze
    self.w = 2
  end

  def bboom
    b
    self.b = 3
    b.nope
  end

  # A HAND-WRITTEN reader keeps its frame -- only the generated shape is
  # iseq-less.
  def hand
    hand_reader.nope
  end

  def hand_reader = @a
end

begin; Holder.new.boom; rescue NoMethodError => e; p e.backtrace.first(2); end
begin; Holder.new.wboom; rescue FrozenError => e; p e.backtrace.first(2); end
begin; Holder.new.bboom; rescue NoMethodError => e; p e.backtrace.first(2); end
begin; Holder.new.hand; rescue NoMethodError => e; p e.backtrace.first(2); end
begin; Holder.new.a(1); rescue ArgumentError => e; p [e.message, e.backtrace.first]; end
frozen = Holder.new.freeze
begin; frozen.b = 9; rescue FrozenError => e; p e.backtrace.first(2); end
