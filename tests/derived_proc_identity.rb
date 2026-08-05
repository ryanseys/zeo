# Two kinds of proc that are DERIVED from something else keep a trace of what
# they came from, and zeo's do not.
#
# 1. `method(:m).to_proc` reports the METHOD's `source_location`; zeo answers
#    nil, the C-level-Proc answer, because the location does not travel from
#    the Method object into the proc it becomes.
#
# 2. `:sym.to_proc` inspects as `#<Proc:0xADDR(&:sym) (lambda)>` -- the symbol
#    is named right in the identity, with no space before it. zeo prints the
#    plain locationless form, so a table of symbol-derived callbacks is again
#    a column of identical strings, which is what
#    `tests/proc_inspect_identity.rb` fixed for ordinary blocks.
#
# A block written in the source now carries both (that file is the control
# below), so what is missing is only the derived cases: the proc is built by a
# runtime row, which has nowhere to put either fact.

def norm(s) = s.sub(/0x\h+/, "0xADDR").sub(%r{[^ ]*/}, "")

def target = :body
mp = method(:target).to_proc
p mp.source_location&.last
p mp.lambda?

sp = :upcase.to_proc
puts norm(sp.inspect)
p sp.source_location
p sp.call("ok")

# An ordinary block carries both -- already correct.
pr = proc { }
p pr.source_location&.last
puts norm(pr.inspect)
