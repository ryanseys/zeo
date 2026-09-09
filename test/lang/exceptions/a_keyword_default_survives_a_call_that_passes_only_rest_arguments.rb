# `def fn(*opts, ivar: false)` called with two positionals leaves ivar false.
# (spinel issue #3204)
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
