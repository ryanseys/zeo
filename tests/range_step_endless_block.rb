r001 = []; (1..).step(3) { |x001| break if x001 > 10; r001 << x001 }; p r001
a002 = (1..); r002 = []; a002.step(2) { |x002| break if x002 > 5; r002 << x002 }; p r002
r004 = []; (1..).each { |x004| break if x004 > 5; r004 << x004 }; p r004
r006 = []; (1..10).step(3) { |x006| r006 << x006 }; p r006
r003 = ((..5).step(2) { |x003| x003 } rescue $!.class); p r003
r007 = ((1..10).step(-1) { |x| x } rescue $!.class); p r007
r008 = []; (1...10).step(3) { |x| r008 << x }; p r008
