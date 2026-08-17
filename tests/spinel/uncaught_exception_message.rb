# An uncaught exception's tail format. In a --debug build the raising frame and
# its callers are printed in front of it (#3974); an ordinary build has no frame
# symbols, so the message alone is what it says.
def boom(v)
  v.length
end

begin
  boom(nil)
rescue NoMethodError => e
  p e.message
end

p "before"
boom(nil)
