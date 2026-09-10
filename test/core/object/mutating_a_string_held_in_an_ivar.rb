# It shares with local aliases and containers, instances stay independent, and
# a read through the reader is a copy.
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
__END__
"aaa!"
true
"bbb!"
true
"aaa!+"
"bbb!"
"AAA!+"
5
"start-x"
true
"START-X"
"b1"
"b1"
"b1?"
