def cls; begin; yield; rescue => e; e.class; end; end
p(cls { Time.utc(2020,13,1) })
p(cls { Time.utc(2020,1,32) })
p Time.utc(2020,2,30).month
p(cls { Time.utc(2020,1,1,25) })
p Time.utc(2020,12,31,23,59,60).year
p(cls { Time.local(2020,0,1) })
p Time.utc(2020,2,29).day
p Time.new(2020,6,15,10,30,0).hour
p(cls { Time.utc(2020,1,1,23,60) })
p(cls { Time.utc(2020,1,1,23,59,61) })
