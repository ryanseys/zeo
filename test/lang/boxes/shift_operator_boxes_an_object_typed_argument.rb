# `junk << Trash.new(1)` on a Poly receiver: the numeric-op fallback's
# match scrutinee must box the unboxed `Arc<Concrete>` argument
# (found by the conformance corpus's argv_gc as a FAIL_RUSTC).

class Trash
  def initialize(n)
    @n = n
  end
  attr_reader :n
end
def build
  junk = []
  junk << Trash.new(7)
  junk
end
puts build.length
puts build[0].n
__END__
1
7
