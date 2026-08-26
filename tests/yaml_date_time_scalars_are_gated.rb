# A bare date or timestamp scalar is a TYPED value psych gates behind
# `permitted_classes` -- plain `safe_load` RAISES Psych::DisallowedClass
# for `2001-12-14`, and with `[Time]` permitted the timestamp loads as a
# real Time. zeo answers plain strings in every case. (Found by the
# 2026-08-24 probe sweep.)
require "yaml"
require "date"
def show
  p yield
rescue Psych::DisallowedClass => e
  puts "#{e.class}: #{e.message}"
end
show { YAML.safe_load("d: 2001-12-14") }
show { YAML.safe_load("t: 2001-12-14 21:59:43.10 -5") }
show { YAML.safe_load("t: 2001-12-14 21:59:43 -05:00", permitted_classes: [Time]) }
show { YAML.safe_load("d: 2001-12-14", permitted_classes: [Date]) }
