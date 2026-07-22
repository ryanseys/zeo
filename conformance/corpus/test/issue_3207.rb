class Crash
  attr_accessor :exit
  def fn
    value = exit
    p value
  end
end
Crash.new.fn

