require "date"

d = Date.new(2024, 1, 31)
p d.next_month.to_s
p d.prev_month.to_s
p d.next_year.to_s
p d.prev_year.to_s
p d.next_month(2).to_s
__END__
"2024-02-29"
"2023-12-31"
"2025-01-31"
"2023-01-31"
"2024-03-31"
