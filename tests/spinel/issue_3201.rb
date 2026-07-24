require "ostruct"
class Main
  def parse(args)
    return 0 if args.empty?
    OpenStruct.new({name: "lee"})
  end
end
object = Main.new.parse(["--name", "lee"])
object[:name] = "bob"
raise unless object.name == "bob"
puts "ok"
