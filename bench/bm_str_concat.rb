# yjit-bench: str_concat - string concatenation performance
# Ported from https://github.com/Shopify/yjit-bench

def concat_test(n, str_to_add)
  s = ""
  i = 0
  while i < n
    s << str_to_add
    i = i + 1
  end
  s
end

result = concat_test(10240, "ssssssee")
puts result.length
