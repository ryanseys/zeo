require "ostruct"

class Main
  def parse(flag)
    return 0 if flag

    result
  end

  def result
    options = OpenStruct.new
    options[:name] = "lee"
    options
  end
end

options = Main.new.parse(false)
raise unless options[:name] == "lee"
puts "ok3193"
