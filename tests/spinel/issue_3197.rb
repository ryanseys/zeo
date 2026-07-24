require "ostruct"

class Crash
  def foo(bool)
    return 123 if !bool
    OpenStruct.new({name: "lee"})
  end
end

object = Crash.new.foo(true)
raise unless object.name == "lee"
puts "ok3197"
