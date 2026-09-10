# Issue #831: punctuation global variables compile to safe
# C identifiers. Pre-fix the generated C had `gv_!` / `gv_;` /
# `gv_,` / `gv_/` etc. which aren't valid C and broke any program
# that referenced them — including `rescue => e` bodies because
# the parser implicitly threads `$!` through some shapes.
puts $!.inspect       # nil outside rescue
puts $;.inspect       # nil (the split-default separator)
puts $,.inspect       # nil (the print separator)
puts $/.inspect       # "\n" — the only one with a meaningful default
__END__
nil
nil
nil
"\n"
