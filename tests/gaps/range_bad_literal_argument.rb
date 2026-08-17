# GAP -- imported from the spinel corpus at c55d9bdb.
# A Range built from a bad literal PANICS the runtime
# (crates/zeo-rt/src/builtins/enumerable.rs) instead of raising TypeError.
#
r = ((1..5).cover?("x") rescue $!.class)
p r

r = ((1..5).include?("x") rescue $!.class); p r          # Ruby: false       Spinel: C compile abort
r = ((1..5).member?("x") rescue $!.class); p r           # Ruby: false       Spinel: C compile abort
r = ((1..5) === "x" rescue $!.class); p r                # Ruby: false       Spinel: C compile abort
r = ((1..5).first("x") rescue $!.class); p r             # Ruby: TypeError   Spinel: C compile abort
r = ((1..5).each_slice("x") { |s| } rescue $!.class); p r # Ruby: TypeError  Spinel: C compile abort

r = ((1..5).eql?(1.0..5.0) rescue $!.class); p r          # Ruby: false       Spinel: C compile abort

r = ((1..5) == (1.0..5.0) rescue $!.class); p r
r = ((1..5).cover?(3) rescue $!.class); p r
r = (("a".."e").include?("c") rescue $!.class); p r
r = ((1..5).take(2) rescue $!.class); p r
r = ((1..5).eql?(1..5) rescue $!.class); p r
