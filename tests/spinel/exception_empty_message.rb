r001 = (begin; raise; rescue RuntimeError => e001; [e001.class.to_s, e001.message]; end); p r001
p RuntimeError.new.message
r2 = (begin; raise "x"; rescue => e; e.message; end); p r2
r3 = (begin; raise RuntimeError; rescue => e; e.message; end); p r3
r4 = (begin; raise ArgumentError, ""; rescue => e; e.message; end); p r4
