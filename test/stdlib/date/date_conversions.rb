require "date"

d = Date.new(2024, 2, 29)
p d.to_date.to_s
p d.to_datetime.to_s
p d.to_time.strftime("%Y-%m-%d %H:%M:%S")
__END__
"2024-02-29"
"2024-02-29T00:00:00+00:00"
"2024-02-29 00:00:00"
