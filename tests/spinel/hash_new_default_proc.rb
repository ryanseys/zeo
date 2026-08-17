r001 = (Hash.new(0).default_proc rescue $!.class); p r001
p(Hash.new.default_proc)
p(Hash.new("x").default_proc)
h004 = Hash.new(0)
p(h004.default_proc)
p({}.default_proc)
h = Hash.new { |hh, k| hh[k] = k.to_s }
p h.default_proc.class
