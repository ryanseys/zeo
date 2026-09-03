# Reading a class variable that was never assigned raises NameError
# ("uninitialized class variable @@missing in ..."). zeo's cvar read answers
# nil instead.
class CvarHost
  def self.read
    @@missing
  rescue NameError => e
    "NameError: #{e.message}"
  end
end

puts CvarHost.read.inspect
puts defined?(@@top_missing).inspect
__END__
"NameError: uninitialized class variable @@missing in CvarHost"
nil
