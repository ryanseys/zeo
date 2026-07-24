# An OpenStruct member read through a poly value must work even when a user
# class defines a method of the same name: the OpenStruct pre-arm keys on
# cls_id and coexists with the user arms (#3264).
require "ostruct"
class Main
  def name = "main-name"
  def parse(h)
    options = h
    return OpenStruct.new(options)
    nil
  end
end
o = Main.new.parse({"name" => "lee"})
p o.name
