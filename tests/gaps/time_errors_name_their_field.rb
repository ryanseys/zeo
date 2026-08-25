# Time's argument errors carry CRuby's exact texts: an out-of-range
# component names the FIELD ("mon out of range", "mday out of range"), and
# `Time + Time` says "time + time?". zeo answers generic messages. (Found
# by the 2026-08-24 probe sweep; matches the TODO(plan P-B) markers in
# builtins/time.rs.)
def show
  yield
rescue StandardError => e
  puts "#{e.class}: #{e.message}"
end
show { Time.utc(2001, 13, 1) }
show { Time.utc(2001, 1, 0) }
show { t = Time.at(0); t + t }
