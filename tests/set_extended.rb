require "set"

# subtract / replace (in place, return self)
s = Set[1, 2, 3, 4]
p s.subtract([2, 3]).to_a.sort
p s.replace([9, 8, 8, 7]).to_a.sort

# flatten / flatten!
nested = Set[Set[1, 2], Set[3, Set[4]]]
p nested.flatten.to_a.sort
f = Set[Set[1, 2], Set[3]]
p f.flatten!.to_a.sort
p Set[1, 2].flatten!

# map! / collect!
m = Set[1, 2, 3]
m.map! { |x| x * 10 }
p m.to_a.sort

# select! / filter! / keep_if
sel = Set[1, 2, 3, 4]
p sel.select! { |x| x.even? }.to_a.sort
p Set[2, 4].select! { |x| x.even? } # no change -> nil
kept = Set[1, 2, 3, 4]
p kept.keep_if { |x| x.odd? }.equal?(kept)

# reject! / delete_if
rej = Set[1, 2, 3, 4]
p rej.reject! { |x| x.even? }.to_a.sort
p Set[1, 3].reject! { |x| x.even? } # no change -> nil
del = Set[1, 2, 3, 4]
p del.delete_if { |x| x.even? }.equal?(del)

# classify
p Set[1, 2, 3, 4, 5].classify { |x| x % 3 }.transform_values { |v| v.to_a.sort }

# divide (one-arg groups by value; two-arg strongly-connected components)
p Set[1, 2, 3, 4].divide { |i| i % 3 }.map { |grp| grp.to_a.sort }.sort
p Set[1, 2, 3, 4].divide { |x, y| (x - y).abs == 1 }.map { |grp| grp.to_a.sort }.sort
