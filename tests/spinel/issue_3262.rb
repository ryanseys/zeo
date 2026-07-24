def content_type(path)
  File.extname(path)
end
def get_path(str)
  x = str.split(" ")
  return x[1] if x[0] == "GET"
  nil
end
p content_type(get_path("GET /index.html"))
p File.basename(get_path("GET /a/b/c.txt"))
p File.dirname(get_path("GET /a/b/c.txt"))
