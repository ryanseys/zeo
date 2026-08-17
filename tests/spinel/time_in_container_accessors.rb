a001 = [Time.utc(2020, 1, 2)]
p a001[0].tv_sec
p a001[0].to_r
h003 = { k: Time.utc(2020, 1, 2) }
p h003[:k].tv_sec
t = Time.utc(2020, 1, 2)
p t.tv_sec
p t.to_r
p a001[0].to_i
p a001[0].year
