cols = [[1, 2], [3, 4]].transpose
p cols[0].sum
r1 = (cols[0].reduce(0) { |a, x| a + x } rescue $!.class); p r1
p cols[1].inject { |a, x| a + x }
p [[1,2],[3,4]].transpose[1].reduce(10) { |a, x| a + x }
