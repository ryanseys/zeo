class Config
  autoload :Mirror, File.expand_path("mirror", __dir__)
end
puts Config::Mirror.name
