module TypedStore
  def self.parse(serialized)
    h = {}
    raw = serialized.to_s
    h["k"] = if raw == "true"
    elsif raw.length >= 2
      raw[1, raw.length - 2].to_s
    else
      raw
    end
    h
  end

  def self.write(serialized)
    parse(serialized)
  end
end

module Seeder
  def self.seed(stream)
    stream.write("")
  end
end

p TypedStore.write("abc")
p TypedStore.write("true")
p TypedStore.write("x")
