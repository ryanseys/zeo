# `\w` in a regex should only match ASCII word characters ([a-zA-Z0-9_]) by
# default in CRuby's onig engine, excluding accented/Unicode letters. zeo's
# `\w` is Unicode-aware and matches "e" with an accent too.
p "café bar".scan(/\w+/)
