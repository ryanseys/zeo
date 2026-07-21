# Hash#default_proc= with a lambda literal (KieranP #2371)
h = {}
h.default_proc = ->(hh, k) { k.to_s }
p h[:x]
h2 = { a: 1 }
h2.default_proc = ->(hh, k) { 99 }
p h2[:zzz]
p h2[:a]
h3 = { "a" => 1 }
h3.default_proc = ->(hh, k) { "miss:" + k }
p h3["q"]
p h3["a"]
