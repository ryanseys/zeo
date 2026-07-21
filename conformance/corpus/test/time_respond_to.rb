t = Time.utc(2026, 7, 16, 13, 45, 30)
p t.respond_to?(:no_such_method)
p t.respond_to?(:succ)
p t.respond_to?(:year)
