# A native class can be subclassed when it has a payload shape to inherit --
# csv's parser is `class Scanner < StringScanner` with a `@keeps` stack of its
# own on top of the inherited scanning behaviour.
require "strscan"

class Keeper < StringScanner
  alias_method :scan_all, :scan

  def initialize(*args)
    super
    @keeps = []
  end

  def keep_start = @keeps.push(pos)

  def keep_end
    start = @keeps.pop
    string.byteslice(start, pos - start)
  end

  def keep_back = (self.pos = @keeps.pop)
end

k = Keeper.new("hello world")
p k.class, k.is_a?(StringScanner), Keeper.superclass

k.scan(/hello/)
k.keep_start
k.scan(/ world/)
p k.keep_end
p k.pos, k.eos?, k.rest
p k.instance_variables

# The inherited surface is all there, including the capture reads.
k2 = Keeper.new("k=v")
p k2.scan_all(/(\w+)=(\w+)/)
p k2[1], k2[2], k2.captures
k2.keep_start
p k2.eos?

# ...and `keep_back` rewinds through the subclass's own state.
k3 = Keeper.new("abcdef")
k3.scan(/abc/)
k3.keep_start
k3.scan(/def/)
p k3.pos
k3.keep_back
p k3.pos, k3.rest
