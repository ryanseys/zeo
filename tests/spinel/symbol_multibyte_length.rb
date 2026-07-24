# Symbol#length / #size count characters, not bytes, for multibyte
# symbol names.
p :αβγ.length
p :café.size
p :abc.length
p :"".size rescue p :abc.size
