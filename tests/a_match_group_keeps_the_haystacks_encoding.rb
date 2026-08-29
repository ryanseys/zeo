binary = "café".b
latin1 = "\xe9".force_encoding("ISO-8859-1")

def show(label)
  puts format("%-32s %s", label, (yield).inspect)
rescue => e
  puts format("%-32s %s", label, e.class)
end

show("group keeps haystack") { /ca(f)/.match(binary)[1].encoding.to_s }
show("group keeps latin1")   { /(.)/.match(latin1)[1].encoding.to_s }
show("whole match")          { /ca(f)/.match(binary)[0].encoding.to_s }
show("pre_match")            { /ca(f)/.match(binary).pre_match.encoding.to_s }
show("post_match")           { /ca(f)/.match(binary).post_match.encoding.to_s }
