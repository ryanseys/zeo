# The class-method channel takes the same rule. It is counted apart from the
# instance channel, so a fix that only watched one would pass everything
# above and still hand `def self.x` the wrong body.

def cpu = ["arm", "64"].join
class C
  [:tag].each { |n| attr_accessor(n) }
  if cpu == "universal"
    ONLY = "universal"
    def self.which = ONLY
  end
end
class C
  def self.which = "real"
end
puts C.which
__END__
real
