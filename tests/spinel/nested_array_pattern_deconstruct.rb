S001 = Struct.new(:a, :b)
arr001 = [S001.new(1, 2)]
r001 = (case arr001; in [S001[x001, y001]] then [x001, y001]; end rescue $!.class); p r001
r002 = (case arr001; in [[p1, p2]] then [p1, p2]; end rescue $!.class); p r002
r003 = (case S001.new(3, 4); in S001[u, v] then [u, v]; end rescue $!.class); p r003
r004 = (case [[1, 2]]; in [[m, n]] then [m, n]; end rescue $!.class); p r004
