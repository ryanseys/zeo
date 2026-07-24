require "yaml"

# load_stream returns every document in a multi-document string.
stream = <<~YAML
  ---
  name: alice
  role: admin
  ---
  name: bob
  role: user
YAML
p Psych.load_stream(stream)

# The block form yields each document in turn.
names = []
Psych.load_stream(stream) { |doc| names << doc["name"] }
p names

# load_file reads and parses a file's first document.
path = "/tmp/sp_psych_example.yaml"
File.write(path, "fruits:\n  - apple\n  - banana\ncount: 2\n")
p Psych.load_file(path)
File.delete(path)
