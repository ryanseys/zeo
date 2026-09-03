# The block captures `self` as a plain `RubyValue::Class`, so the ivar
# resolves through `ivar_get_dyn`/`ivar_set_dyn`'s Class arm rather than
# the static class-id path -- the two must agree on the same storage.

class Blk
  @vals = []
  def self.collect
    [1, 2, 3].each { |i| @vals << i * 10 }
    @vals
  end
end
p Blk.collect
__END__
[10, 20, 30]
