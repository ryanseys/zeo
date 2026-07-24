class Config
  attr_accessor :name
end
config = Config.new
config.name = "app"
p "app".sub(config.name, config.name)
p "banana".gsub(config.name, config.name)
class C2; attr_accessor :pat, :rep; end
c = C2.new; c.pat = "o"; c.rep = "0"
p "foo".sub(c.pat, c.rep)
p "foo".gsub(c.pat, c.rep)
