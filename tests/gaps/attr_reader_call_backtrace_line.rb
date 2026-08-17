class Holder
  attr_reader :a

  def initialize = @a = 1

  def boom
    a.nope
  end
end

begin
  Holder.new.boom
rescue NoMethodError => e
  p e.backtrace.first
end
