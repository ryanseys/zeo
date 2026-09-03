module Shellish
  def self.escape(s)
    s.gsub(" ", "\\ ")
  end
end
