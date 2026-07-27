# A String reached through a poly binding landing in a `const char *` slot.
# The File / Dir / IO surface took its path arguments with emit_expr, so a
# String ivar the fixpoint had widened -- or any untyped accessor -- went in as
# a raw sp_RbVal and the generated C did not compile. Three separate sightings
# (#3256, #3330, #3385) were the same shape at different call sites.
class Store
  def initialize(path)
    @path = path
  end

  # @path is widened to poly by the nil write below, so every use of it is a
  # boxed value rather than a const char *.
  def forget
    @path = nil
  end

  def write_it(text)
    File.open(@path, "w") { |f| f.write(text) }
  end

  # an empty block: the call's value still has to fit the slot it is typed into
  def touch
    File.open(@path, "a") { |f| }
  end

  def exists = File.exist?(@path)
  def sized = File.size(@path)
  def dir = File.directory?(@path)
  def base = File.basename(@path)
  def full = File.expand_path(@path)
  def read_it = File.read(@path)
  def drop = File.delete(@path)
end

path = "spinel_pathslot_#{Process.pid}.tmp"
s = Store.new(path)
s.write_it("hello")
p s.exists
p s.sized
p s.dir
p s.base == path
p s.full.end_with?(path)
p s.read_it
s.touch
p s.sized
s.drop
p s.exists
