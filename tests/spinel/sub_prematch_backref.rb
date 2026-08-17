# The replacement backreferences for the text around the match: \` is what
# came before it and \' what comes after. Neither was expanded, so both were
# copied through as the literal two characters.
p "hello".sub(/l/, '\`')
p "hello".sub(/l/, "\\'")
p "hello".gsub(/l/, '\`')
p "hello".sub(/l/, '[\`|\0|\']')
p "hello".sub(/l/, '\0')
p "hello".sub(/(l)(o)/, '\2\1')
