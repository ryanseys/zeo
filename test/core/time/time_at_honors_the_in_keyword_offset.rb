# `Time.at(epoch, in: offset)` attaches a display utc_offset to the
# absolute instant (no shift, unlike Time.new's local components).

p Time.at(0, in: "+09:00").utc_offset
p Time.at(0, in: "-05:00").utc_offset
p Time.at(0, in: 3600).utc_offset
__END__
32400
-18000
3600
