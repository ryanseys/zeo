t001 = Time.utc(2024, 2, 29, 12, 0, 0)
p t001.deconstruct_keys([:year])
case t001
in { year: 2024 }
  p :matched
else
  p :no_match
end
case t001
in { year: 1999 } then p :y
else p :n
end
r = (t001 in { year: 2024 }); p r
r2 = (t001 in { year: 1999 }); p r2
