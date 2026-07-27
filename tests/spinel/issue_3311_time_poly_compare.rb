p(Time.new(2026, 1, 2) <=> Time.new(2026, 1, 1))
a = [Time.new(2026, 1, 2), Time.new(2026, 1, 1)]
p(a[0] <=> a[1])
r1 = (a.min.day rescue $!.class); p r1
r2 = (a.sort.map(&:day) rescue $!.class); p r2
r3 = (a.sort_by { |t| t }.map(&:day) rescue $!.class); p r3
