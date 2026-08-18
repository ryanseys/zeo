require "date"

p DateTime.parse("2024-01-02T03:04:05+00:00").to_s
p DateTime.strptime("2024-01-02T03:04:05+00:00", "%Y-%m-%dT%H:%M:%S%z").to_s
p DateTime.new(2024, 1, 2, 3, 4, 5).to_s
p DateTime.parse("2024-01-02T03:04:05+00:00").hour
