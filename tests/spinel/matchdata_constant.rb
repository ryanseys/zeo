# `MatchData` names a class every match already answers to #class, but the
# constant itself was not registered and reading it raised NameError.
p MatchData
p MatchData.name
p "ab".match(/a/).class
p("ab".match(/a/).is_a?(MatchData))
p MatchData.to_s
