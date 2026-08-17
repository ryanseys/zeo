h001 = { a: 1, b: 2 }
r001 = (h001.each { |_k001, _v001| h001[:c] = 3 } rescue $!.class)
p r001
h002 = { a: 1, b: 2 }
h002.each { |k, _v| h002.delete(k) }
p h002
h003 = { a: 1, b: 2 }
h003.each { |k, v| h003[k] = v * 10 }
p h003
h4 = { "a" => 1, "b" => 2 }
r4 = []; h4.each { |k, v| r4 << [k, v] }; p r4
h5 = { a: 1, b: 2, c: 3 }
r5 = []; h5.each { |k, v| r5 << k }; p r5
p h5
