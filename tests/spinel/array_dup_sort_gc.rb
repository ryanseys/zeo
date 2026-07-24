# Array#dup and Array#sort copy the receiver into a freshly allocated array.
# That allocation can trigger a collection, so the receiver must stay rooted
# across it -- otherwise a receiver that is only reachable as the call's
# transient (e.g. the result of a method, not a named local) is freed mid-copy
# and the copy reads freed memory. Exercised across every array kind under GC
# pressure; verified clean under -fsanitize=address with SPINEL_GC_STRESS=1.
def make_ints;  (1..200).to_a.reverse;            end
def make_flts;  (1..200).map { |i| i.to_f }.reverse; end
def make_strs;  (1..200).map { |i| "s#{1000 - i}" };  end
def make_polys; (1..200).map { |i| i.even? ? i : "x#{i}" }; end

GC.start
p make_ints.sort.first
p make_ints.dup.length
GC.start
p make_flts.sort.first
p make_strs.sort.first
GC.start
p make_polys.dup.length
