# Three bodies for one name, two of them guarded: the two lists differ by
# two, so an off-by-one fix would still land on the wrong row.

def cpu = ["arm", "64"].join
class C
  [:tag].each { |n| attr_accessor(n) }
  if cpu == "universal"
    def which = "first guard"
  end
  if cpu == "sparc"
    def which = "second guard"
  end
  def which = "real"
end
puts C.new.which
__END__
real
