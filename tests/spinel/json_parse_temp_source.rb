# The parser walks its input in place while allocating every container and
# string it builds, so a document handed straight in -- `JSON.parse
# File.read(path)`, or any expression whose result nothing else holds -- has to
# stay rooted for the whole walk. It did not: the first collection under the
# parse left the cursor reading freed memory, which reads as a parse error
# whose message varies run to run, and as a segfault once the document is large
# enough. Sized so the parse itself collects; smaller documents finish before
# the first one and prove nothing.
require "json"
require "strscan"

def build(n)
  parts = []
  i = 0
  while i < n
    parts << "{\"id\":" + i.to_s + ",\"name\":\"feature-" + i.to_s +
             "\",\"tags\":[\"a\",\"b\",\"c\"],\"pos\":[-63.121807,46.233965]}"
    i = i + 1
  end
  "[" + parts.join(",") + "]"
end

# the source is a temp: nothing but the argument slot holds it across the parse
doc = JSON.parse(build(80000))
p doc.length
p doc[0]["id"]
p doc[0]["name"]
p doc[79999]["name"]
p doc[1234]["tags"]
p doc[1234]["pos"]

# a scanner over a temp source borrows the same way
sc = StringScanner.new(build(50))
p sc.scan(/\[\{"id":\d+/)
