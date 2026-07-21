module Config
  class << self
    attr_accessor :env_slot
  end
  def self.fetch
    v = env_slot
    v.nil? ? "default" : v
  end
end
p Config.fetch
Config.env_slot = "production"
p Config.fetch

class Settings
  class << self
    attr_accessor :level, :tier
  end
  def self.summary
    "#{level} / #{tier}"
  end
end
Settings.level = 3
Settings.tier = "prod"
puts Settings.summary

# a bare local assignment stays a local
class Mix
  class << self
    attr_accessor :slot
  end
  def self.compute
    slot = 99
    slot
  end
end
p Mix.compute
