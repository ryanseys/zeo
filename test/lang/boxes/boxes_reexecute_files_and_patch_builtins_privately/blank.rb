class String
  def blank?
    strip.empty?
  end
end
class Foo
  def self.blank_one?
    "   ".blank?
  end
end
class Counter
  @@count = 0
  def self.bump
    @@count += 1
  end
  def self.count
    @@count
  end
end
puts "loaded"
