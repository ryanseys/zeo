ENV["TZ"] = "America/Los_Angeles"
p Time.local(0).utc_offset
p Time.local(2024, 1, 1).utc_offset
