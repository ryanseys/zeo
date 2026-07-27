# Shared-mutable strings, ivar slots (P4): an in-place-mutated ivar string
# shares its handle with local aliases and containers; multiple instances
# stay independent; external attr reads are safe copies.
class Doc
  attr_reader :title
  def initialize(t)
    @title = t
    a = @title
    @title << "!"
    p a
    p a.equal?(@title)
  end
  def bump = @title << "+"
  def shout = @title.upcase!
end
d1 = Doc.new(+"aaa")
d2 = Doc.new(+"bbb")
d1.bump
p d1.title
p d2.title
d1.shout
p d1.title
p d1.title.length

# Toplevel ivar
@log = +"start"
snap = @log
@log << "-x"
p snap
p snap.equal?(@log)
box = [@log]
@log.upcase!
p box[0]

# hand-written reader over a promoted ivar
class Memo
  def initialize
    @body = +"b"
    keep = @body
    @body << "1"
    p keep
  end
  def body = @body
end
m = Memo.new
p m.body
p "#{m.body}?"
