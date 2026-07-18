# Range#size handles integer, endless, and Float-ended ranges.
p (1..10).size
p (1...10).size
p (1..).size            # endless -> Infinity
p (1..7.5).size         # Float end floors
p (1...7.5).size
p (5..1).size           # empty -> 0

# A Float or absent begin can't be sized (CRuby raises TypeError).
r1 = ((1.5..5).size rescue $!.class); p r1
r2 = ((..5).size rescue $!.class); p r2

# Proc.new builds a proc from its block (or a forwarded &block).
double = Proc.new { |n| n * 2 }
p double.call(21)
def forward(&b) = Proc.new(&b)
p forward { |n| n + 1 }.call(9)
