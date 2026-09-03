# A call to an accessor from INSIDE the class was inlined to the ivar read,
# and a later `define_method` of that name did not reach it. The same call
# from outside saw the redefinition, so one object answered two different
# values for one method depending on who asked.
#
# `inline_accessor` (`clif/expr.rs`) devirtualizes a self-call whose target
# `accessor_shape` recognizes: `attr_reader :v`, `attr_writer`, and a `Struct`
# member. The fold now asks `zeo_rt_is_live` first and takes the ordinary
# dispatch once anything has been defined at run time.
#
# The guard is emitted only for a name `may_be_patched_at_runtime` answers
# for, so a program that redefines nothing emits what it emitted before --
# which is what keeps `bm_getivar`/`bm_setivar`/`bm_attr_accessor` unchanged.
# The writer half stands down instead of guarding: both arms would lower the
# argument from the same node.
#
# rspec's summary line is the case that found it. The golden stubs
# `SummaryNotification#duration` with `define_method(:duration) { 0.0 }`; the
# stub worked from outside and the formatter's own `format_duration(duration)`
# still read the member.

class K
  attr_reader :v
  def initialize = @v = 1
  def inside = v
end

k = K.new
p k.inside
K.class_eval { define_method(:v) { 99 } }
p k.inside
p k.v

S = Struct.new(:duration)
class S
  def report = "took #{duration}"
end
s = S.new(1.5)
p s.report
S.class_eval { define_method(:duration) { 0.0 } }
p s.report
p s.duration
__END__
1
99
99
"took 1.5"
"took 0.0"
0.0
