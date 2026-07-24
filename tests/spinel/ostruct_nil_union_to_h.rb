# to_h on an OpenStruct|nil union value: the poly to_h keeps the boxed value
# for a poly consumer and handles the OpenStruct member table (#3282).
require "ostruct"
class Main
  def parse(flag)
    return OpenStruct.new(name: "lee") if flag
    nil
  end
end
options = Main.new.parse(true)
raise "FAIL" if options.to_h.key?(:help)
p options.to_h.key?(:name)
h = options.to_h
p h[:name]
puts "OK"
