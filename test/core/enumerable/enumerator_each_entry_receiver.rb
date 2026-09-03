v = (1..3).each.each_entry { |x| x }
p v
v2 = (1..5).each.each_slice(2) { |s| s }
p v2
v3 = [1, 2, 3].each.each_entry { |x| x }
p v3
v4 = [1, 2, 3].each_entry { |x| x }
p v4
v5 = (1..3).each_entry { |x| x }
p v5
__END__
#<Enumerator: 1..3:each>
#<Enumerator: 1..5:each>
#<Enumerator: [1, 2, 3]:each>
[1, 2, 3]
1..3
