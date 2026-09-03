# Bundler's actual nesting: a guard inside a guard, with a local assigned
# between them and the body reading a constant only the inner branch writes.

def cpu = ["arm", "64"].join
class C
  [:tag].each { |n| attr_accessor(n) }
  if cpu
    arch = cpu
    if arch == "universal"
      DEEP = "deep"
      def which = DEEP
    end
  end
  def which = "real"
end
puts C.new.which
__END__
real
