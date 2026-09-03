# ...and with the guarded body written last, which is the same rule read
# from the other end.

def cpu = ["arm", "64"].join
class C
  [:tag].each { |n| attr_accessor(n) }
  def which = "real"
  if cpu == "universal"
    def which = "universal"
  end
end
puts C.new.which
__END__
real
