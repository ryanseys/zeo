# The reported shape, smallest form: guarded body FIRST, unguarded second.

def cpu = ["arm", "64"].join
class C
  [:tag].each { |n| attr_accessor(n) }
  if cpu == "universal"
    ONLY = "universal"
    def which = ONLY
  end
end
class C
  def which = "real"
end
puts C.new.which
puts C.const_defined?(:ONLY)
__END__
real
false
