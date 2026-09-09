# Reading it answers the ivar rather than calling Kernel#exit.
# (spinel issue #3207)
class Crash
  attr_accessor :exit
  def fn
    value = exit
    p value
  end
end
Crash.new.fn

__END__
nil
