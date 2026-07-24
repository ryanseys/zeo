P = Data.define(:x, :y)
r1 = (begin; P.new(1); "no error"; rescue ArgumentError; "argerror"; end); p r1
r2 = (begin; P.new; "no error"; rescue ArgumentError; "argerror"; end); p r2
r3 = (begin; P.new(x: 1, y: 2, z: 3); "no error"; rescue ArgumentError; "argerror"; end); p r3
r4 = (begin; P.new(1, y: 2); "no error"; rescue ArgumentError; "argerror"; end); p r4
