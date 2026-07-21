t = Time.utc(2026, 7, 16, 13, 45, 30)
p t.deconstruct_keys([:year, :month, :day])
p t.deconstruct_keys([:hour, :min, :sec])
