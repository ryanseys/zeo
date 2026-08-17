D001 = Data.define(:a, :b)
arr001 = [D001.new(1, 2)]
r001 = (case arr001; in [D001[x001, y001]] then [x001, y001]; end rescue $!.class)
p r001

r002 = (case arr001; in [[x002, y002]] then [x002, y002]; end rescue $!.class); p r002

r003 = (case arr001; in [D001(a: x003, b: y003)] then [x003, y003]; end rescue $!.class); p r003  # => [1, 2]
r004 = (case arr001; in [D001 => e004] then e004.a; end rescue $!.class); p r004                  # => 1
r005 = (case D001.new(1, 2); in [x005, y005] then [x005, y005]; end rescue $!.class); p r005      # => [1, 2]
p D001.new(1, 2).deconstruct                                                                      # => [1, 2]

D9 = Data.define(:a, :b)
r = (D9.new(3, 4).to_a rescue $!.class); p r
r = (D9.new(3, 4).deconstruct rescue $!.class); p r
S9 = Struct.new(:a, :b)
r = (case [S9.new(5, 6)]; in [S9[u, v]] then [u, v]; end rescue $!.class); p r
