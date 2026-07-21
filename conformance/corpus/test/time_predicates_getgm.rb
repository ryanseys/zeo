t = Time.utc(2026, 7, 16, 13, 45, 30)   # Thursday
p t.sunday?; p t.thursday?; p t.saturday?
p t.eql?(t); p t.eql?(Time.utc(2020,1,1)); p t.eql?(5)
x = t.getgm; p x.year
