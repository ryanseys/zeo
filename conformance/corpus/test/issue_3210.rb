class Foo
  def initialize
    @key = :enabled
  end
end
class Crash
  def key = @key ||= :url
end
Foo.new
raise unless Crash.new.key == :url
puts "ok"
