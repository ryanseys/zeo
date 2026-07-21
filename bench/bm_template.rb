# Simple template engine
# Replaces {{key}} with values from a hash

def render(template, vars)
  result = ""
  i = 0
  while i < template.length
    if i + 1 < template.length && template[i] == "{" && template[i + 1] == "{"
      # Find closing }}
      j = i + 2
      while j + 1 < template.length
        if template[j] == "}" && template[j + 1] == "}"
          break
        end
        j = j + 1
      end
      key = template[i + 2, j - i - 2]
      if vars.has_key?(key)
        result = result + vars[key]
      else
        result = result + "{{" + key + "}}"
      end
      i = j + 2
    else
      result = result + template[i]
      i = i + 1
    end
  end
  result
end

# Test
tmpl = "Hello {{name}}! You are {{age}} years old. Welcome to {{city}}."
vars = {"name" => "Matz", "age" => "58", "city" => "Matsue"}
puts render(tmpl, vars)

tmpl2 = "{{greeting}} {{name}}, {{message}}"
vars2 = {"greeting" => "Hi", "name" => "World", "message" => "how are you?"}
puts render(tmpl2, vars2)

# Missing key
puts render("{{known}} and {{unknown}}", {"known" => "yes"})

# Benchmark
i = 0
while i < 50000
  render(tmpl, vars)
  i = i + 1
end
puts "done"
