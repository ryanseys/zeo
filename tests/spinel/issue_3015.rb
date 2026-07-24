m = "abc".match(/(?<x>b)(?<y>c)/)
p m.deconstruct_keys([:x])
p m.deconstruct_keys([:y, :x])
p m.deconstruct_keys([:x, :nope])
p m.deconstruct_keys([])
p m.deconstruct_keys(nil)
p m.deconstruct_keys([:x, :y, :z])
case "2024-01".match(/(?<year>\d+)-(?<mon>\d+)/)
in { year:, mon: }
  puts "#{year}/#{mon}"
end
