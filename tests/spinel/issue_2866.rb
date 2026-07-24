t = Time.utc(2026, 7, 16, 13, 45, 30)
p t.deconstruct_keys(nil)
p t.deconstruct_keys([:zone])
