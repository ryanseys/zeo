module ClassMethods
  def label
    @label
  end

  def label=(v)
    @label = v
  end

  def described
    "<#{@label.inspect}>"
  end
end

class Win
  extend ClassMethods
end

class Other
  extend ClassMethods
end

p Win.label
Win.label = "main"
p Win.label
p Win.described
p Other.label
Other.label = "second"
p [Win.label, Other.label]
