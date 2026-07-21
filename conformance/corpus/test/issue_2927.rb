def parse(qs)
  qs.split("&").each_with_object({}) do |pair, params|
    key, value = pair.split("=", 2)
    value = "" if value.nil?
    (params[key] ||= []) << value
  end
end

def build(params)
  params.flat_map { |key, values| values.map { |v| "#{key}=#{v}" } }.join("&")
end

puts build(parse("a=1&b=2"))
p build({ "q" => ["ruby"] })
