# require "time" is supposed to add Time.parse (and friends) to the core
# Time class, but zeo doesn't define it -- only Time.at/Time.now etc. work.
require "time"
p Time.parse("2024-01-15 10:30:00 UTC").to_s
