# `def fn(*opts, ivar: false)` called with two positionals leaves ivar false.
class Crash
  attr_reader :ivar
  def fn(*opts, ivar: false)
    @ivar = ivar
  end
end
c = Crash.new
c.fn("foo", "bar")
raise if c.ivar
puts "ok"
__END__
ok
