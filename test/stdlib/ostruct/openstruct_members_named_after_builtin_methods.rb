# `uniq` and `size` as members answer their values, not Object's methods, and
# uniq still works on a plain array beside it.
# (spinel issue #3341)
require "ostruct"

class Flag
  attr_reader :switches
  def initialize(opts)
    @switches = opts
    raise ArgumentError, "duplicate switch" unless switches.uniq.length == switches.length
  end
end

class Config
  def str(*opts) = Flag.new(opts)
end

c = Config.new
p c.str("-a", "--alpha").switches
p c.str("-b").switches.uniq
r = (c.str("-x", "-x").switches rescue $!.class); p r
o = OpenStruct.new(uniq: "member", size: 7)
p o.uniq
p o.size
__END__
["-a", "--alpha"]
["-b"]
ArgumentError
"member"
7
