# An assignment written INSIDE an argument list parses -- and executes --
# before the call's own block body, so the name is the enclosing scope's
# local and the block shares it. The deep shape is activerecord's
# `attribute_types.fetch(name = result.columns[i]) { lookup(name) }`: the
# argument assignment lives in an enclosing BLOCK's scope and a nested
# escaping block reads it.
def lookup(map)
  [nil, "b", nil].map do |column|
    map.fetch(key = column.to_s) do
      fallback(key)
    end
  end
end

def fallback(key)
  "fell back for #{key.inspect}"
end

p lookup("b" => "hit")

# One level shallower: the argument assignment feeds the same call's block
# directly at method scope.
def direct(map)
  map.fetch(k = "absent") { "#{k} missing" }
end

p direct({})
__END__
["fell back for \"\"", "hit", "fell back for \"\""]
"absent missing"
