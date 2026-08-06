# securerandom picks its implementation with a `begin/rescue` inside `class <<
# self`, aliasing whichever one works; and `class << self; self; end` is the
# classic idiom whose VALUE is the singleton class.

module Source
  class << self
    def from_device
      "device"
    end

    def from_fallback
      "fallback"
    end

    begin
      raise RuntimeError, "no device here"
      alias generate from_device
    rescue RuntimeError
      alias generate from_fallback
    end
  end

  SINGLETON = class << self
    self
  end
end

puts Source.generate
puts Source::SINGLETON == Source.singleton_class
puts Source::SINGLETON.instance_method(:generate).owner == Source.singleton_class
