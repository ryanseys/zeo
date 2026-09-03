# `Zlib::Inflate` is a value-builtin payload root: a subclass wraps the
# native stream as its payload and adds ivars of its own -- `Inflate.new`
# with no arguments is a real empty form, so a subclass without its own
# `initialize` seats one directly, and one WITH an `initialize` re-seats
# through `super`. The single biggest subclassing ask in the gem-probe
# ledger (35 gems).
require "zlib"

deflated = Zlib::Deflate.deflate("hello, payload root")

class TaggedInflate < Zlib::Inflate
  def initialize(tag)
    @tag = tag
    super()
  end

  attr_reader :tag
end

inf = TaggedInflate.new("gzip-ish")
puts inf.class
puts inf.tag
puts inf.inflate(deflated)
inf.close

puts TaggedInflate.new("t").is_a?(Zlib::Inflate)
__END__
TaggedInflate
gzip-ish
hello, payload root
true
