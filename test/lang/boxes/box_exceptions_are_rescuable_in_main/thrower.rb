class BoxError < StandardError
end
class Thrower
  def self.go
    raise BoxError, "from the box"
  end
end
