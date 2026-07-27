class Flag
  attr_reader :switches, :help
  def initialize(opts)
    @help = if opts.last && opts.last[0] != "-"
      opts.pop
    end
    @switches = opts
    raise ArgumentError, "duplicate switch" unless switches.uniq.length == switches.length
  end
end
class Config
  def str(*opts) = Flag.new(opts)
  def builtin = Flag.new(["-h", "--help"] + ["Show this"])
end
c = Config.new
p c.str("--name").switches
p c.builtin.switches
p c.str("-a", "-a", "x").switches rescue p $!.class
