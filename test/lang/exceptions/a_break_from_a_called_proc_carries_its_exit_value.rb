# A `break` in a proc called after its defining scope is a LocalJumpError whose exit_value is the break's argument.
# (spinel issue #3024)
r = begin; pr = proc { break 5 }; pr.call; rescue LocalJumpError => e; e.exit_value; end
p r
r2 = begin; pr = proc { break }; pr.call; rescue LocalJumpError => e; [e.reason, e.exit_value]; end
p r2
r3 = begin; pr = proc { break "str" }; pr.call; rescue LocalJumpError => e; e.exit_value; end
p r3
__END__
5
[:break, nil]
"str"
