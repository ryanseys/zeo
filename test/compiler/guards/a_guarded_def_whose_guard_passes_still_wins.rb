# A guard that PASSES still installs. Without this the rule could be
# satisfied by never installing a guarded body at all, which is a different
# bug wearing the same green.

def cpu = ["arm", "64"].join
class C
  [:tag].each { |n| attr_accessor(n) }
  def which = "real"
  def self.which = "real class method"
  if cpu == "arm64"
    def which = "taken"
    def self.which = "taken class method"
  end
end
puts C.new.which
puts C.which
__END__
taken
taken class method
