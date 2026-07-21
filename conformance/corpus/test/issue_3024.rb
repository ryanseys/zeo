r = begin; pr = proc { break 5 }; pr.call; rescue LocalJumpError => e; e.exit_value; end
p r
r2 = begin; pr = proc { break }; pr.call; rescue LocalJumpError => e; [e.reason, e.exit_value]; end
p r2
r3 = begin; pr = proc { break "str" }; pr.call; rescue LocalJumpError => e; e.exit_value; end
p r3
