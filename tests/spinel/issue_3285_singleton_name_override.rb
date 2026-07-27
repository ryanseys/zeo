module Foo
  def self.name
    "custom"
  end
end
puts Foo.name

module Bar
  def self.to_s
    "bar-to-s"
  end

  def self.describe
    "#{to_s}/#{name}"
  end
end
puts Bar.to_s
puts Bar.name
puts Bar.describe

class Baz
  def self.name
    "baz-name"
  end

  def self.own
    name
  end
end
puts Baz.name
puts Baz.own

class Plain
end
puts Plain.name
