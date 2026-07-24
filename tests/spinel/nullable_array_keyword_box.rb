# A nil-defaulting array-typed value boxed into a poly slot must become nil,
# not a truthy OBJ wrapping a NULL pointer (segfault on later access) (#3275).
class Config
  def int(*opts, choices: nil) = Flag.new(choices:)
end
class Flag
  def initialize(choices: nil)
    @choices = choices
    validate
  end
  def validate
    "choices must be empty" if @choices && @choices.empty?
  end
end
c = Config.new
c.int("--http-port")
c.int "--bad-int", choices: ["1"]
puts "ok"
