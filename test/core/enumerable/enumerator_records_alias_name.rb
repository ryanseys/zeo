p [1, 2].collect.inspect
p [1, 2].filter.inspect
p [1, 2].find_all.inspect
p({ a: 1 }.each_pair.inspect)
p({ a: 1 }.keep_if.inspect)
__END__
"#<Enumerator: [1, 2]:collect>"
"#<Enumerator: [1, 2]:filter>"
"#<Enumerator: [1, 2]:find_all>"
"#<Enumerator: {a: 1}:each_pair>"
"#<Enumerator: {a: 1}:keep_if>"
