# A STRING pattern must be encoding-compatible with the receiver --
# CRuby's `rb_enc_check`, the same test `+` makes. Without it the
# pattern's bytes were reinterpreted under the receiver's encoding and
# could MATCH, so a substitution happened where ruby raises.
#
# CORRECTION to this file's original note, which said the regexp rows
# raise: they do not. `/x/` is US-ASCII and EUC-JP is ASCII-compatible, so
# ruby simply does not match, and the first three rows below record that
# -- they agreed all along. Only the String-pattern row was wrong.
e = "日".encode("EUC-JP")
def show
  p yield
rescue Exception => ex
  puts "#{ex.class}: #{ex.message}"
end
show { e =~ /x/ }
show { e.match?(/x/) }
show { e.match(/x/) }
show { "日".encode("Shift_JIS").gsub("日".encode("EUC-JP"), "x") }
__END__
nil
false
nil
Encoding::CompatibilityError: incompatible character encodings: Shift_JIS and EUC-JP
