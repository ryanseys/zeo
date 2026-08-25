# `String#force_encoding` on a frozen string raises FrozenError -- even
# when the new encoding equals the current one. zeo answers the string
# silently: force_encoding skips the frozen guard every other mutator
# has. (Found by the 2026-08-24 probe sweep.)
f = "x".freeze
def show
  p yield
rescue Exception => e
  puts "#{e.class}: #{e.message}"
end
show { f.force_encoding("ASCII-8BIT") }
show { f.force_encoding("UTF-8") }
p f.encoding.name
