tbl001 = { even: ->(x) { x.even? } }
p(case 4 when tbl001[:even] then "y" else "n" end)
tbl002 = { even: ->(x) { x.even? } }; p(tbl002[:even] === 4)
arr003 = [->(x) { x.even? }]; p(arr003[0] === 4)
l = ->(x) { x.odd? }
p(case 3 when l then "o" else "e" end)
p(Integer === 3)
p((1..5) === 3)
