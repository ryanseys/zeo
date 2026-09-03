p require("did_you_mean")
p defined?(DidYouMean)
p DidYouMean::SpellChecker.new(dictionary: %w[length size]).correct("lenght")
__END__
true
"constant"
["length"]
