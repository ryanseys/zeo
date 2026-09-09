# `build(*args, **kwargs)` passes both through, with and without keywords.
# (spinel issue #3176)
def build(name, **extra)
  "#{name}/#{extra.size}"
end
def wrapper(*args, **kwargs)
  build(*args, **kwargs)
end
puts wrapper("dave", role: "dev")
puts wrapper("amy", role: "dev", team: "x")
puts wrapper("joe")
__END__
dave/1
amy/2
joe/0
