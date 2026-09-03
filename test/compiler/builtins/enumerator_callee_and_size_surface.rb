# An Enumerator records the ALIAS actually called (CRuby captures
# `__callee__`), and its size comes from the size function the METHOD supplies
# -- so `[1, 2].each.size` is 2 while `[1, 2].to_enum(:each).size` is nil.
p [1,2].each.size, [1,2].map.size, [1,2].select.size, [1,2].each_slice(2).size, [1,2].each_cons(2).size
p [1,2].to_enum(:each).size, [1,2].enum_for(:each).size, [1,2].enum_for(:each) { 9 }.size
p 3.times.size, 3.times.to_enum.size
p [1,2].cycle.size, [1,2].cycle(3).size, [].cycle.size, [1,2].cycle(0).size
p [1,2].map.inspect, [1,2].collect.inspect, [1,2].select.inspect, [1,2].filter.inspect, [1,2].find_all.inspect
p [1,2].reject.inspect, [1,2].each_entry.inspect
p({ a: 1 }.each.inspect)
p({ a: 1 }.each_pair.inspect)
p({ a: 1 }.select!.inspect)
p({ a: 1 }.filter!.inspect)
p({ a: 1 }.keep_if.inspect)
p({ a: 1 }.reject!.inspect)
p({ a: 1 }.delete_if.inspect)
p [1,2].map!.inspect, [1,2].collect!.inspect, [1,2].select!.inspect, [1,2].filter!.inspect, [1,2].keep_if.inspect
p [1,2].flat_map.inspect, [1,2].collect_concat.inspect
p [1,2].detect.inspect, [1,2].find.inspect
p [1,2].each_with_index.size, [1,2].each_with_object(0).size
p [1,2].map { |x| x * 2 }, [1,2].collect { |x| x }, [1,2].select { |x| x > 1 }, [1,2].find_all { |x| x > 1 }
p({ a: 1, b: 2 }.select { |k, v| v > 1 })
p [1,2,3].select! { |x| x > 1 }
h = {a: 1, b: 2}; h.keep_if { |k,v| v > 1 }; p h
__END__
2
2
2
1
1
nil
nil
9
3
nil
Infinity
6
0
0
"#<Enumerator: [1, 2]:map>"
"#<Enumerator: [1, 2]:collect>"
"#<Enumerator: [1, 2]:select>"
"#<Enumerator: [1, 2]:filter>"
"#<Enumerator: [1, 2]:find_all>"
"#<Enumerator: [1, 2]:reject>"
"#<Enumerator: [1, 2]:each_entry>"
"#<Enumerator: {a: 1}:each>"
"#<Enumerator: {a: 1}:each_pair>"
"#<Enumerator: {a: 1}:select!>"
"#<Enumerator: {a: 1}:filter!>"
"#<Enumerator: {a: 1}:keep_if>"
"#<Enumerator: {a: 1}:reject!>"
"#<Enumerator: {a: 1}:delete_if>"
"#<Enumerator: [1, 2]:map!>"
"#<Enumerator: [1, 2]:collect!>"
"#<Enumerator: [1, 2]:select!>"
"#<Enumerator: [1, 2]:filter!>"
"#<Enumerator: [1, 2]:keep_if>"
"#<Enumerator: [1, 2]:flat_map>"
"#<Enumerator: [1, 2]:collect_concat>"
"#<Enumerator: [1, 2]:detect>"
"#<Enumerator: [1, 2]:find>"
2
2
[2, 4]
[1, 2]
[2]
[2]
{b: 2}
[2, 3]
{b: 2}
