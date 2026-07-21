r = "a=b\nc=d".each_line.each_with_object({}) do |raw, config|
  key, value = raw.split("=", 2)
  config[key] = value
end
p r
