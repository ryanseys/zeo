# A call to an accessor from INSIDE the class is inlined to the ivar read, and
# a later `define_method` of that name does not reach it. The same call from
# outside sees the redefinition, so one object answers two different values
# for one method depending on who asks.
#
# `inline_accessor` (`clif/expr.rs`) devirtualizes a self-call whose target
# `accessor_shape` recognizes: `attr_reader :v`, `attr_writer`, and a `Struct`
# member, which is the same thing on a slot `instance_variables` does not
# report. The fold has no guard, so a runtime install of the same name is
# invisible to every site already inlined.
#
# rspec's summary line is the case that found it. The golden stubs
# `SummaryNotification#duration` with `define_method(:duration) { 0.0 }` to
# take the wall clock out of the output; the stub works from outside and the
# formatter's own `format_duration(duration)` -- a self-call in the same class
# -- still reads the member. `tests/rspec_end_to_end.rb` is one line off
# byte-identical because of it, and nothing else.
#
# THE FIX IS THE ONE `a_later_def_on_a_builtin_reaches_back.rb` DESCRIBES, on
# a different table: a per-(class, name) switch the inlined site reads, false
# until a runtime install stores true. It is the same trade -- one relaxed
# load on a hot path -- and the same reason it is filed rather than done: an
# accessor read is the hottest shape zeo emits, and `bm_getivar`/`bm_setivar`
# exist to measure exactly this. It wants its own pass with a number.
#
# What must NOT be done instead: `mark_live()`. That is a GLOBAL gate taking
# every send in the program onto the per-ancestor walk, and gems install
# methods at runtime routinely.

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
