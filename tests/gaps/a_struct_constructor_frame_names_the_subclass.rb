# A `Data` subclass's constructor reports two frames wrong, and its `Struct`
# twin reports both right -- which is what narrows this.
#
# CRuby, raising from inside `D2#initialize` after `super`:
#
#   ...:29:in 'D2#initialize'
#   ...:34:in 'D.new'
#
# zeo says `Object#initialize` for the first and skips the second entirely.
# The same shape on a `Struct` subclass comes out byte-identical, so neither
# half is about subclassing or about the compiled body: the minted class's
# `new` is a native row (`rstruct::struct_construct`) that dispatches
# `initialize` through `send_in`, and on the Data route that dispatch neither
# pushes a frame for the constructor it stands in for nor carries the
# receiver's class down to the body.
#
# The BODY is right. It runs, with the right arguments, and raises where CRuby
# raises -- `tests/a_data_freezes_inside_its_own_initialize.rb` pins that.
# Only the backtrace is wrong.
#
# `frames::synthetic_c_frame` is the hook the C-method frames already use, and
# this is the same shape for a native constructor. Filed rather than fixed
# because a frame push on a constructor is a hot path -- `bm_object_new` and
# `bm_object_new_init` exist to measure exactly this, and the C-frame work
# recorded that a naive push once cost `ruby_xor` 0.996s -> 3.8s. It wants a
# pass with a number, like its two siblings.

D = Data.define(:m)

class D2 < D
  def initialize(**kw)
    super
    @z = 1
  end
end

begin
  D2.new(m: 1)
rescue => e
  puts e.backtrace.first(2).map { |l| l.sub(/\A.*?([^\/]+\.rb)/, '\1') }
end

S = Struct.new(:a)
class S2 < S
  def initialize(*args)
    super
    raise "from the subclass"
  end
end
begin
  S2.new(1)
rescue => e
  puts e.backtrace.first(2).map { |l| l.sub(/\A.*?([^\/]+\.rb)/, '\1') }
end
