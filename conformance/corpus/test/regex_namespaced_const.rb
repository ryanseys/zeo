# Refs #1008. A regex in a namespaced constant must resolve like a top-level
# one: gsub/sub/scan/=~ should dispatch to the engine, not silently no-op
# (gsub) or raise undefined-method (scan).
module M
  RE = /a/
end
puts "banana".gsub(M::RE, "X")
puts "banana".scan(M::RE).length

# nested namespace + .freeze
module A
  module B
    WS = /\s+/.freeze
  end
end
puts "x  y   z".gsub(A::B::WS, "-")
puts "x  y   z".scan(A::B::WS).length
