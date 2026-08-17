# A fixed-offset Time has no zone NAME: CRuby answers nil where the empty
# string the broken-down form leaves behind came back.
p Time.utc(2020, 6, 15, 12, 0, 0).getlocal("+05:30").zone
p Time.utc(2020, 6, 15, 12, 0, 0).getlocal(19800).zone
p Time.at(0, in: "+09:00").zone
p Time.new(2020, 1, 1, 0, 0, 0, "+09:00").zone
p Time.utc(2020, 1, 1).zone
p Time.utc(2020, 1, 1).utc?
p Time.at(0, in: "+09:00").utc_offset
p Time.new(2020, 1, 1, 0, 0, 0, "+09:00").utc_offset
