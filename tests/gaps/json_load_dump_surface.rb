# JSON.dump/load surface: dump with a LIMIT raises ArgumentError beyond
# it; dump to an IO WRITES to it (zeo returns "" and writes nothing --
# silent data loss); load(nil) answers nil; load's proc argument visits
# every parsed object; JSON.parse! exists (NaN/Infinity allowed); and
# `allow_nan: true` admits the literals to parse. (Found by the
# 2026-08-24 probe sweep.)
require "json"
require "stringio"
def show
  p yield
rescue Exception => e
  puts "#{e.class}: #{e.message}"
end
show { JSON.dump([[[1]]], 2) }
show { io = StringIO.new(+""); JSON.dump({ a: 1 }, io); io.string }
show { JSON.load(nil) }
show do
  seen = []
  JSON.load('{"a":[1]}', ->(o) { seen << o.class })
  seen.map(&:to_s).sort
end
show { JSON.parse!("[NaN, Infinity]").map { |f| f.nan? || f.infinite? } }
show { JSON.parse("[NaN]", allow_nan: true).first.nan? }
