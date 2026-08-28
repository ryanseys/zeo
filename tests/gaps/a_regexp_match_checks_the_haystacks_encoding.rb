utf8 = "café"
binary = utf8.b
latin1 = "\xe9".force_encoding("ISO-8859-1")

def show(label)
  puts format("%-32s %s", label, (yield).inspect)
rescue => e
  puts format("%-32s %s", label, e.class)
end

show("fixed re vs binary")     { /café/ =~ binary }
show("fixed re match binary")  { /café/.match(binary) }
show("fixed re vs latin1")     { /café/ =~ latin1 }
show("ascii re vs binary")     { /caf/ =~ binary }
show("group keeps haystack")   { /ca(f)/.match(binary)[1].encoding.to_s }
show("group keeps latin1")     { /(.)/.match(latin1)[1].encoding.to_s }
show("String#=~ binary")       { binary =~ /café/ }
show("scan keeps haystack")    { binary.scan(/./).first.encoding.to_s }
show("split keeps haystack")   { binary.split(/f/).first.encoding.to_s }
show("ascii re on latin1")     { latin1.sub(/z/, "y").encoding.to_s }
