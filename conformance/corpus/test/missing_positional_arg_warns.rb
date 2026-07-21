# A statically-detectable wrong-arity call raises ArgumentError at RUNTIME,
# exactly when the call would run (dead code stays silent, matching CRuby).
# This replaces the old compat behaviour of padding the missing slot with 0.

module M
  def self.run!(a, b, c, d)
    puts "a=#{a} b=#{b} c=#{c} d=#{d}"
  end
end

begin
  M.run!(1, 2, false)        # missing `d`
rescue ArgumentError => e
  puts "ArgumentError: #{e.message}"
end
M.run!(10, 20, true, 30)     # all args supplied

def never_called
  M.run!(1)                  # dead code: no raise unless executed
end
puts "done"
