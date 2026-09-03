# `Proc#inspect` identifies the proc: its address, where it was written, and
# whether it is a lambda -- `#<Proc:0x000...7318 /path/to/file.rb:3>`, with
# ` (lambda)` appended for one. zeo answers a bare `#<Proc>` for every proc in
# the program, so no two are distinguishable.
#
# The information is all present: zeo already reports `Proc#source_location`
# and `#lambda?` correctly (both below), and every other heap value's `inspect`
# carries an address. Only the assembled string is missing.
#
# It matters where procs are data rather than control flow -- a callback
# registry, a middleware stack, a `Hash` of handlers -- because `p handlers`
# then prints a column of identical `#<Proc>` with nothing to tell them apart.

pr = proc { }
la = lambda { }

p pr.lambda?
p la.lambda?
p pr.source_location&.last
p la.source_location&.last

puts pr.inspect.sub(/0x\h+/, "0xADDR").sub(/[^ ]+\.rb/, "FILE")
puts la.inspect.sub(/0x\h+/, "0xADDR").sub(/[^ ]+\.rb/, "FILE")

# Two distinct procs must not inspect identically.
a = proc { }
b = proc { }
p a.inspect == b.inspect

p :sym.to_proc.lambda?
p method(:puts).to_proc.lambda?
__END__
false
true
14
15
#<Proc:0xADDR FILE:14>
#<Proc:0xADDR FILE:15 (lambda)>
false
true
true
