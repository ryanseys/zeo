r001 = (begin; a001 = [1].freeze; a001 << 2; rescue FrozenError => e001; e001.class; end)
p r001

r002 = (begin; a002 = 1; a002; rescue => e002; e002.class; end); p r002   # => 1
r003 = (begin; a003 = [1]; a003; rescue; 0; end); p r003                  # => [1]
p(begin; a004 = [1]; a004; rescue; 0; end)                                # => [1]
r005 = ([1].freeze << 2 rescue $!.class); p r005                          # => FrozenError

r = (begin; a = [1]; a << 2; rescue => e; 99; end); p r
r = (begin; a = [1]; a.push(2); rescue => e; e.class; end); p r
r = (begin; s = "x".freeze; s << "y"; rescue FrozenError => e; e.class; end); p r
r = (begin; h = { k: 1 }; h.size; rescue => e; e.class; end); p r
