class Win
  class << self
    attr_reader :shown
    attr_accessor :title
    alias_method :shown?, :shown
  end

  module ClassMethods
    def check
      shown? ? "yes" : "no"
    end

    def named
      title.nil? ? "(none)" : title
    end
  end
  extend ClassMethods
end

p Win.check
p Win.named
Win.title = "hello"
p Win.named
p Win.shown?
p Win.title
