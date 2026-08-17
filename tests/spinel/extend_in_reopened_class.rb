# `extend M` written in a REOPENING of the class: the module's methods are
# transplanted from every body that defines the class, not only the first
# (#3802).
class Win
  def self.shown?
    false
  end

  module ClassMethods
    def check
      shown? ? 'yes' : 'no'
    end

    def viewport_width
      100
    end
  end
end

class Win
  extend ClassMethods
end

puts Win.check
puts Win.viewport_width

# the same module extended in the original body still works
class Dlg
  def self.open?
    true
  end

  module CM
    def state
      open? ? 'open' : 'shut'
    end
  end

  extend CM
end

puts Dlg.state
