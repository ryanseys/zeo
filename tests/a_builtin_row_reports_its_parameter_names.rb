# `Method#parameters` on a native row used to answer the anonymous descriptor
# derived from its arity -- `[[:req]]` where ruby says `[[:req, :fmt]]`. zeo
# reported a parameter NAME on no builtin row at all.
#
# The DSL now spells a row's signature (`params "fmt, buffer: nil"`), which is
# ruby's own `def` spelling, and the ARITY falls out of it -- so the two
# answers cannot disagree for a row that carries one. A keyword had no
# spelling in the DSL before at all, because a native body receives one inside
# the options Hash it already takes as a positional slot: what the body
# RECEIVES and what ruby REPORTS are different questions.
p Array.instance_method(:pack).parameters
p Array.instance_method(:first).parameters
p Array.instance_method(:sample).parameters
p String.instance_method(:unpack1).parameters
p IO.instance_method(:read_nonblock).parameters
p Dir.method(:glob).parameters
p Marshal.method(:load).parameters
p Time.method(:at).parameters
p TracePoint.instance_method(:enable).parameters

# The arity a spelling implies is the one ruby derives from the same signature.
p GC.method(:start).parameters
p GC.method(:start).arity
# ... and a `|`-joined sibling keeps its own, which is why the spelling is
# per-NAME rather than per-def.
p GC.method(:compact).parameters
p GC.method(:compact).arity

# An anonymous forwarding slot is named for its own sigil, ruby's own answer.
p Ractor.instance_method(:send).parameters

# A hand-registered exception row reaches no arity table, so zeo's two
# catch-alls answered it differently: `-1` through `#arity` and `[]` through
# `#parameters`. Every one of them is declared now, beside its ownership mark.
p Exception.instance_method(:set_backtrace).parameters
p Exception.instance_method(:set_backtrace).arity
p Exception.instance_method(:full_message).parameters
p Exception.instance_method(:message).parameters
p NameError.instance_method(:receiver).arity
p SystemExit.instance_method(:success?).arity
