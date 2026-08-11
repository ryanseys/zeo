# `defined?(@@x)` answers at RUNTIME: nil before the first assignment, then
# "class variable" -- a static answer breaks every runs-once guard of the
# shape rspec's ensure_example_groups_are_configured uses.
class Config
  def self.check
    unless defined?(@@configured)
      puts "configuring"
      @@configured = true
    end
    puts "checked: #{defined?(@@configured).inspect}"
  end
end

p defined?(@@nowhere)
Config.check
Config.check
