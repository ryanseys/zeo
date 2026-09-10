# Referencing a constant the program never defines, on purpose, behind a rescue
# that catches the NameError is how a program probes for one -- ruby/spec
# does exactly this, and nothing is said about it at any stage. A
# defined-nowhere warning belongs to a reference nothing catches, and still
# fires there.
p(begin; NoSuchConstA; rescue NameError => e; e.class.to_s; end)

r = begin
  NoSuchConstB
rescue => e
  e.class.to_s
end
p r

c = (NoSuchConstC rescue "caught")
p c

def probe
  NoSuchConstD
rescue NameError
  "rescued in a method"
end
p probe

# a nested rescue whose OUTER clause is the one that catches it
p(begin
    begin
      NoSuchConstE
    rescue TypeError
      "wrong one"
    end
  rescue NameError
    "outer caught it"
  end)
__END__
"NameError"
"NameError"
"caught"
"rescued in a method"
"outer caught it"
