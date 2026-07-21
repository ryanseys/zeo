def build(name, **extra)
  "#{name}/#{extra.size}"
end
def wrapper(*args, **kwargs)
  build(*args, **kwargs)
end
puts wrapper("dave", role: "dev")
puts wrapper("amy", role: "dev", team: "x")
puts wrapper("joe")
