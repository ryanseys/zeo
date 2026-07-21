t = Time.utc(2026, 1, 1); a = Time.utc(2000, 1, 1); b = Time.utc(2030, 1, 1)
p t.between?(a, b)
p t.clamp(a, b).year
